use serde::Serialize;

/// Stable error vocabulary shared with the frontend. The React layer maps
/// these to user-facing copy; Rust never sends raw HTTP strings as kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// 401/403 — token missing or wrong.
    Auth,
    /// 413 — recording exceeded the server's upload limit.
    TooLong,
    /// 502/504 from the ASR API (upstream engine failure) — retryable once.
    Upstream,
    /// Other 4xx/5xx we don't special-case.
    Server,
    /// Connection refused / DNS failure — stack down or cold-booting.
    Offline,
    /// Request timed out (cold model load can take minutes).
    Timeout,
    /// Local audio pipeline problem (device, capture, encode).
    Audio,
    /// Global shortcut registration failed (conflict / permissions).
    Hotkey,
}

/// Error type returned from commands and events. `message` is safe to show.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("{1}")]
    Mapped(ErrorKind, String),
    #[error("audio error: {0}")]
    Audio(String),
}

impl ClientError {
    pub fn audio(msg: impl Into<String>) -> Self {
        ClientError::Audio(msg.into())
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            ClientError::Mapped(k, _) => *k,
            ClientError::Audio(_) => ErrorKind::Audio,
        }
    }

    /// 502/504-style upstream failures are worth one automatic retry after a short delay.
    pub fn retryable(&self) -> bool {
        matches!(self, ClientError::Mapped(ErrorKind::Upstream, _))
    }
}

impl Serialize for ClientError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("ClientError", 2)?;
        s.serialize_field("kind", &self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

/// Map an HTTP failure to a typed error, preferring the server's own
/// `{"error":{"message":…}}` / `{"detail":…}` body when parseable.
pub fn map_http_status(status: reqwest::StatusCode, body: &str) -> ClientError {
    let kind = match status.as_u16() {
        401 | 403 => ErrorKind::Auth,
        413 => ErrorKind::TooLong,
        502 | 504 => ErrorKind::Upstream,
        408 => ErrorKind::Timeout,
        400..=499 => ErrorKind::Server,
        _ => ErrorKind::Upstream,
    };
    let message = extract_error_message(body).unwrap_or_else(|| match kind {
        ErrorKind::Auth => "the API token was rejected — check Settings".to_string(),
        ErrorKind::TooLong => "recording is too large for the server limit".to_string(),
        ErrorKind::Upstream => format!("upstream ASR error ({})", status.as_u16()),
        ErrorKind::Timeout => "the server timed out".to_string(),
        _ => format!("server error ({})", status.as_u16()),
    });
    ClientError::Mapped(kind, message)
}

/// Best-effort extraction of `{"error":{"message":...}}` / `{"detail":...}` bodies.
pub fn extract_error_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    if let Some(m) = v.pointer("/error/message").and_then(|m| m.as_str()) {
        return Some(m.to_string());
    }
    if let Some(d) = v.get("detail").and_then(|d| d.as_str()) {
        return Some(d.to_string());
    }
    None
}
