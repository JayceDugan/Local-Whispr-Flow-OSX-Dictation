//! Dictation cycle orchestration: hotkey → capture → upload → inject.
//!
//! This module is the single owner of the recording lifecycle. The React
//! layer receives `asr://status` snapshots and never drives the loop itself.

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::asr;
use crate::audio::{Recorder, StartInfo};
use crate::config::AppConfig;
use crate::error::{ClientError, ErrorKind};
use crate::inject;
use crate::keychain;
use crate::tray;

/// Serialized as the `asr://status` event payload and returned by
/// `get_status`. The frontend renders purely from this snapshot.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrStatus {
    pub phase: PhaseWire,
    pub health: HealthWire,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_result: Option<LastResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorWire>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PhaseWire {
    Idle,
    Recording,
    Processing,
    Error,
    ServerStarting,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HealthWire {
    Online,
    Offline,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastResult {
    pub text: String,
    pub raw_text: String,
    pub cleanup_applied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub latency_ms: u64,
    /// Client-measured slow path — likely cold engine or network, not inference.
    pub slow: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorWire {
    pub kind: ErrorKind,
    pub message: String,
}

impl Default for AsrStatus {
    fn default() -> Self {
        Self {
            phase: PhaseWire::Idle,
            health: HealthWire::Online,
            last_result: None,
            error: None,
        }
    }
}

/// Mutable session state shared across commands, hotkey callbacks and tasks.
#[derive(Default)]
pub struct Session {
    recorder: Mutex<Option<Recorder>>,
    status: Mutex<AsrStatus>,
}

impl Session {
    pub fn snapshot(&self) -> AsrStatus {
        self.status.lock().clone()
    }

    fn with_status<R>(&self, f: impl FnOnce(&mut AsrStatus) -> R) -> R {
        f(&mut self.status.lock())
    }
}

/// Publish a new phase: update session state, tray icon, menu label, event.
pub fn set_phase(app: &AppHandle, phase: PhaseWire) {
    app.state::<Session>().with_status(|s| {
        s.phase = phase;
        if phase != PhaseWire::Error {
            s.error = None;
        }
    });
    apply_phase_visuals(app, phase);
    emit_status(app);
}

fn apply_phase_visuals(app: &AppHandle, phase: PhaseWire) {
    tray::set_phase(app, phase);
    tray::sync_toggle_label(app, phase == PhaseWire::Recording);
}

pub fn set_health(app: &AppHandle, health: HealthWire) {
    let session = app.state::<Session>();
    let phase_now = session.snapshot().phase;
    // Offline with nothing running surfaces as "server starting"; a live
    // recording/processing keeps its own phase. Error clears once the server
    // is back and idle.
    let effective = match (health, phase_now) {
        (HealthWire::Offline, PhaseWire::Idle | PhaseWire::ServerStarting) => {
            PhaseWire::ServerStarting
        }
        (HealthWire::Online, PhaseWire::ServerStarting | PhaseWire::Error) => PhaseWire::Idle,
        _ => phase_now,
    };
    session.with_status(|s| {
        s.health = health;
        s.phase = effective;
        if effective == PhaseWire::Idle {
            s.error = None;
        }
    });
    apply_phase_visuals(app, effective);
    emit_status(app);
}

pub fn emit_status(app: &AppHandle) {
    let snapshot = app.state::<Session>().snapshot();
    let _ = app.emit("asr://status", snapshot);
}

pub fn current_status(app: &AppHandle) -> AsrStatus {
    app.state::<Session>().snapshot()
}

fn config(app: &AppHandle) -> AppConfig {
    app.state::<crate::state::ConfigHolder>().get()
}

/// Start capture. Errors if already recording or the server is known-down.
/// Gates on the monitor's cached health — a live probe here would add up to
/// seconds of latency to the hotkey path.
pub fn start_recording(app: &AppHandle) -> Result<StartInfo, ClientError> {
    let session = app.state::<Session>();
    if session.recorder.lock().is_some() {
        return Err(ClientError::audio("already recording"));
    }
    if session.snapshot().health == HealthWire::Offline {
        return Err(ClientError::Mapped(
            ErrorKind::Offline,
            "ASR server is not reachable — it may be cold-booting".to_string(),
        ));
    }

    let (recorder, info) = Recorder::start()?;
    *session.recorder.lock() = Some(recorder);
    set_phase(app, PhaseWire::Recording);
    spawn_cap_watchdog(app.clone());
    Ok(info)
}

/// Polls the capture buffer's cap flag; auto-stops at MAX_SECONDS.
fn spawn_cap_watchdog(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let capped = {
                let session = app.state::<Session>();
                let guard = session.recorder.lock();
                match guard.as_ref() {
                    None => return, // stopped normally
                    Some(r) => r.is_capped(),
                }
            };
            if capped {
                stop_and_transcribe(&app);
                return;
            }
        }
    });
}

/// Stop capture and run upload → inject. Returns immediately; results arrive
/// via `asr://status`.
pub fn stop_and_transcribe(app: &AppHandle) {
    let recorder = app.state::<Session>().recorder.lock().take();
    let Some(recorder) = recorder else { return };

    set_phase(app, PhaseWire::Processing);
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        match run_cycle(app2.clone(), recorder).await {
            Ok(outcome) => on_success(&app2, outcome),
            Err(e) => on_failure(&app2, e),
        }
    });
}

async fn run_cycle(app: AppHandle, recorder: Recorder) -> Result<asr::TranscribeOutcome, ClientError> {
    // WAV encode (downmix/resample of minutes of audio) off the async workers.
    let wav = tauri::async_runtime::spawn_blocking(move || recorder.finish_into_wav())
        .await
        .map_err(|e| ClientError::audio(format!("encode task failed: {e}")))??;
    let cfg = config(&app);
    let token = keychain::load_token()?;
    asr::transcribe(&cfg.base_url, token.as_deref(), wav, cfg.cleanup).await
}

fn on_success(app: &AppHandle, outcome: crate::asr::TranscribeOutcome) {
    let t = outcome.transcription.clone();
    // Injection blocks ~60 ms (pasteboard settle). Its failure must not lose
    // the transcript — the result is recorded either way.
    let injected = inject::inject_text(&t.text);
    if let Err(e) = &injected {
        eprintln!("[asr] injection failed: {e}");
    }
    app.state::<Session>().with_status(|s| {
        s.last_result = Some(LastResult {
            text: t.text.clone(),
            raw_text: t.raw_text,
            cleanup_applied: t.cleanup_applied,
            warning: t.warning.clone(),
            latency_ms: outcome.latency_ms,
            slow: outcome.slow,
        });
        match injected {
            Ok(()) => {
                s.phase = PhaseWire::Idle;
                s.error = None;
            }
            Err(e) => {
                s.phase = PhaseWire::Error;
                s.error = Some(ErrorWire {
                    kind: ErrorKind::Audio,
                    message: format!("transcribed, but paste injection failed: {e}"),
                });
            }
        }
    });
    apply_phase_visuals(app, app.state::<Session>().snapshot().phase);
    emit_status(app);
}

fn on_failure(app: &AppHandle, e: ClientError) {
    let kind = e.kind();
    app.state::<Session>().with_status(|s| {
        s.phase = PhaseWire::Error;
        s.error = Some(ErrorWire {
            kind,
            message: e.to_string(),
        });
    });
    apply_phase_visuals(app, PhaseWire::Error);
    emit_status(app);
}

/// Surface an arbitrary failure (e.g. hotkey registration) through status.
pub fn report_error(app: &AppHandle, e: ClientError) {
    on_failure(app, e);
}

/// Toggle entry point used by the hotkey and tray menu.
pub fn toggle(app: &AppHandle) {
    let recording = app.state::<Session>().recorder.lock().is_some();
    if recording {
        stop_and_transcribe(app);
    } else if let Err(e) = start_recording(app) {
        // Start failures (mic denied, server down) surface through status.
        on_failure(app, e);
    }
}
