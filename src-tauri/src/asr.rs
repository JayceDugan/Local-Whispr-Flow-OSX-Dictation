//! HTTP client for the rig's asr-api facade. All networking lives here;
//! the React layer only ever sees typed results via Tauri commands/events.

use std::time::{Duration, Instant};

use reqwest::multipart;
use serde::{Deserialize, Serialize};

use crate::error::{map_http_status, ClientError, ErrorKind};

/// Server per-upstream timeout is 120 s; stay above it so a long recording
/// is never cut off client-side.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(130);
const HEALTH_TIMEOUT: Duration = Duration::from_millis(2500);
/// One automatic retry for 502/504 — the facade is stateless per request.
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// Mirrors the server's /v1/transcribe response 1:1 (verified against the
/// live endpoint): text/raw_text/cleanup_applied/timings_ms (+ warning when
/// cleanup degraded).
#[derive(Debug, Clone, Serialize, Deserialize)]
// Server wire format is snake_case (verified live); TS never sees this struct.
pub struct Transcription {
    /// Cleaned text — what gets injected.
    pub text: String,
    /// Verbatim transcript, kept for a future "as spoken" mode.
    pub raw_text: String,
    pub cleanup_applied: bool,
    #[serde(default)]
    pub warning: Option<String>,
    #[serde(default)]
    pub timings_ms: Timings,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Timings {
    #[serde(default)]
    pub asr: u64,
    #[serde(default)]
    pub cleanup: u64,
    #[serde(default)]
    pub total: u64,
}

/// Result of a full transcribe cycle, including client-measured latency so
/// the UI can distinguish warm-path (~0.3–0.6 s) from cold/network slowness.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeOutcome {
    pub transcription: Transcription,
    /// stop → upload-complete wall time on the client.
    pub latency_ms: u64,
    /// True when the request took unusually long (cold engine / network).
    pub slow: bool,
}

const SLOW_THRESHOLD_MS: u64 = 2500;

/// `GET /healthz` — never authenticated. Err only means "not healthy yet".
pub async fn healthz(base_url: &str) -> Result<(), ClientError> {
    let url = format!("{}/healthz", base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    match client.get(&url).timeout(HEALTH_TIMEOUT).send().await {
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) => Err(ClientError::Mapped(
            ErrorKind::Offline,
            format!("healthz returned {}", resp.status()),
        )),
        Err(e) => Err(ClientError::Mapped(ErrorKind::Offline, e.to_string())),
    }
}

/// `POST /v1/transcribe` with WAV bytes; bearer header only when a token is
/// stored. Retries once on rig-side failure (502/504), then surfaces the
/// server's error.message.
pub async fn transcribe(
    base_url: &str,
    token: Option<&str>,
    wav: Vec<u8>,
    cleanup: bool,
) -> Result<TranscribeOutcome, ClientError> {
    let started = Instant::now();
    let first = attempt(base_url, token, &wav, cleanup).await;
    let transcription = match first {
        Ok(t) => t,
        Err(e) if e.retryable() => {
            tokio::time::sleep(RETRY_DELAY).await;
            attempt(base_url, token, &wav, cleanup).await?
        }
        Err(e) => return Err(e),
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    Ok(TranscribeOutcome {
        transcription,
        latency_ms,
        slow: latency_ms > SLOW_THRESHOLD_MS,
    })
}

async fn attempt(
    base_url: &str,
    token: Option<&str>,
    wav: &[u8],
    cleanup: bool,
) -> Result<Transcription, ClientError> {
    let url = format!("{}/v1/transcribe", base_url.trim_end_matches('/'));
    let part = multipart::Part::bytes(wav.to_vec())
        .file_name("recording.wav")
        .mime_str("audio/wav")
        .map_err(|e| ClientError::audio(e.to_string()))?;
    let mut form = multipart::Form::new().part("file", part);
    if cleanup {
        form = form.text("cleanup", "true".to_string());
    }

    let client = reqwest::Client::new();
    let mut req = client.post(&url).multipart(form).timeout(REQUEST_TIMEOUT);
    if let Some(t) = token.filter(|t| !t.is_empty()) {
        req = req.bearer_auth(t);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            // reqwest folds connect-refused/DNS into `is_connect`; timeouts
            // are distinguishable and map to their own UX kind.
            let kind = if e.is_timeout() {
                ErrorKind::Timeout
            } else {
                ErrorKind::Offline
            };
            let message = if kind == ErrorKind::Timeout {
                "server did not respond in time".to_string()
            } else {
                format!("cannot reach server: {e}")
            };
            return Err(ClientError::Mapped(kind, message));
        }
    };

    let status = resp.status();
    if !status.is_success() {
        // Non-JSON bodies still map by status with a generic message.
        let body = resp.text().await.unwrap_or_default();
        return Err(map_http_status(status, &body));
    }

    // A 200 with a non-JSON body would otherwise be silent — surface it.
    match resp.json::<Transcription>().await {
        Ok(t) => Ok(t),
        Err(e) => Err(ClientError::Mapped(
            crate::error::ErrorKind::Server,
            format!("unexpected response from server: {e}"),
        )),
    }
}
