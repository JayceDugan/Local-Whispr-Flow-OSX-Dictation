// Wire types mirrored from src-tauri (serde camelCase). Keep in sync with
// flow.rs / config.rs — Rust is the source of truth.

export type AsrPhase =
  | "idle"
  | "recording"
  | "processing"
  | "error"
  | "serverStarting";

export type Health = "online" | "offline";

export type ErrorKind =
  | "auth"
  | "tooLong"
  | "upstream"
  | "server"
  | "offline"
  | "timeout"
  | "audio"
  | "hotkey";

export interface AsrError {
  kind: ErrorKind;
  message: string;
}

export interface LastResult {
  text: string;
  rawText: string;
  cleanupApplied: boolean;
  warning?: string;
  latencyMs: number;
  slow: boolean;
}

export interface AsrStatus {
  phase: AsrPhase;
  health: Health;
  lastResult?: LastResult;
  error?: AsrError;
}

export interface AppConfigView {
  baseUrl: string;
  hotkey: string;
  cleanup: boolean;
  /** Keychain presence only — the token value never reaches JS. */
  hasToken: boolean;
}

export interface ConfigPatch {
  baseUrl?: string;
  hotkey?: string;
  cleanup?: boolean;
}

/** User-facing copy per error kind; `message` from Rust is shown alongside. */
export const ERROR_HEADLINE: Record<ErrorKind, string> = {
  auth: "Token rejected",
  tooLong: "Recording too long",
  upstream: "ASR engine error",
  server: "Server error",
  offline: "Server unreachable",
  timeout: "Server timed out",
  audio: "Microphone problem",
  hotkey: "Hotkey conflict",
};
