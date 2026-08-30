//! Floating HUD: a small click-through pill near the bottom of the screen
//! that mirrors the dictation cycle while the user works in another app.
//!
//! Same SPA bundle as the panel; the webview branches on window label "hud".
//! Rust drives visibility (show/hide, reposition) and pushes `asr://hud`
//! events for transient states the status snapshot doesn't cover ("done").

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindowBuilder, WebviewUrl};

pub const HUD_LABEL: &str = "hud";

/// Logical size; positioned bottom-center on every show (displays change).
const HUD_W: f64 = 280.0;
const HUD_H: f64 = 64.0;
const MARGIN_BOTTOM: f64 = 56.0;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HudState {
    Listening,
    Transcribing,
    /// Text injected at the cursor.
    Done,
    /// Anything went wrong (details live in the panel).
    Failed,
}

pub struct Hud {
    visible: AtomicBool,
    /// Generation counter: a newer notify cancels an older auto-hide timer.
    generation: AtomicU64,
}

impl Hud {
    pub fn new() -> Self {
        Self {
            visible: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }
}

/// Create the hidden overlay window. Call from setup (main thread).
pub fn build_hud(app: &AppHandle) -> tauri::Result<()> {
    // Click-through, non-focusable: showing this must never steal keyboard
    // focus from the app the user is dictating into.
    let hud = WebviewWindowBuilder::new(app, HUD_LABEL, WebviewUrl::App("/".into()))
        .title("ASR HUD")
        .inner_size(HUD_W, HUD_H)
        .decorations(false)
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false)
        .visible(false)
        .build()?;
    let _ = hud.set_ignore_cursor_events(true);
    Ok(())
}

fn set_visible(app: &AppHandle, on: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(win) = handle.get_webview_window(HUD_LABEL) else {
            return;
        };
        if on {
            // Bottom-center of the monitor the cursor is on (multi-display).
            let cursor = handle.cursor_position().ok();
            let monitors = handle.available_monitors().unwrap_or_default();
            let monitor = monitors
                .iter()
                .find(|m| {
                    cursor.is_some_and(|c| {
                        let p = m.position();
                        let s = m.size();
                        c.x >= p.x as f64
                            && c.x < p.x as f64 + s.width as f64
                            && c.y >= p.y as f64
                            && c.y < p.y as f64 + s.height as f64
                    })
                })
                .cloned()
                .or_else(|| handle.primary_monitor().ok().flatten())
                .or_else(|| monitors.into_iter().next());
            if let Some(m) = monitor {
                let sf = m.scale_factor();
                let p = m.position();
                let s = m.size();
                let x = p.x as f64 + (s.width as f64 - HUD_W * sf) / 2.0;
                let y = p.y as f64 + s.height as f64 - (HUD_H + MARGIN_BOTTOM) * sf;
                let _ = win.set_position(PhysicalPosition::new(x as i32, y as i32));
            }
            let _ = win.show();
        } else {
            let _ = win.hide();
        }
        if let Some(hud) = handle.try_state::<Hud>() {
            hud.visible.store(on, Ordering::SeqCst);
        }
    });
}

/// Push a transient state to the HUD and manage its visibility.
/// Listening/Transcribing keep the HUD up; Done/Failed auto-hide.
pub fn notify(app: &AppHandle, state: HudState) {
    let _ = app.emit("asr://hud", state);
    set_visible(app, true);

    let hide_after = match state {
        HudState::Listening | HudState::Transcribing => return,
        HudState::Done => std::time::Duration::from_millis(1800),
        HudState::Failed => std::time::Duration::from_millis(2600),
    };

    let gen = app.state::<Hud>().generation.fetch_add(1, Ordering::SeqCst) + 1;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(hide_after).await;
        // A newer notify (e.g. next recording started) owns visibility now.
        if handle.state::<Hud>().generation.load(Ordering::SeqCst) == gen {
            set_visible(&handle, false);
        }
    });
}
