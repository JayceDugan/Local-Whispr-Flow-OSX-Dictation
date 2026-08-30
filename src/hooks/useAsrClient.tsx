import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { AppConfigView, AsrStatus, ConfigPatch } from "@/lib/types";

/**
 * Single source of truth: Rust owns the dictation loop and pushes
 * `asr://status` snapshots. This context only mirrors the latest snapshot
 * and exposes command wrappers — components never touch networking or tokens.
 */
interface AsrClient {
  status: AsrStatus | null;
  config: AppConfigView | null;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  saveConfig: (patch: ConfigPatch) => Promise<void>;
  saveToken: (token: string) => Promise<void>;
  deleteToken: () => Promise<void>;
  closePanel: () => void;
  accessibilityTrusted: boolean | null;
  requestAccessibility: () => Promise<boolean>;
  openAccessibilitySettings: () => void;
}

const AsrContext = createContext<AsrClient | null>(null);

export function AsrProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AsrStatus | null>(null);
  const [config, setConfig] = useState<AppConfigView | null>(null);
  const [accessibilityTrusted, setAccessibilityTrusted] = useState<boolean | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  useEffect(() => {
    let cancelled = false;
    void invoke<AsrStatus>("get_status")
      .then((s) => !cancelled && setStatus(s))
      .catch(console.error);
    // Silent preflight (no system prompt) so the panel can warn about paste failures.
    void invoke<boolean>("accessibility_status", { prompt: false })
      .then((t) => !cancelled && setAccessibilityTrusted(t))
      .catch(console.error);
    void invoke<AppConfigView>("get_config")
      .then((c) => !cancelled && setConfig(c))
      .catch(console.error);
    listen<AsrStatus>("asr://status", (event) => setStatus(event.payload))
      .then((un) => {
        if (cancelled) un();
        else unlistenRef.current = un;
      })
      .catch(console.error);
    return () => {
      cancelled = true;
      unlistenRef.current?.();
    };
  }, []);

  const start = useCallback(async () => {
    await invoke("start_recording");
  }, []);
  const stop = useCallback(async () => {
    await invoke("stop_recording");
  }, []);

  const saveConfig = useCallback(async (patch: ConfigPatch) => {
    setConfig(await invoke<AppConfigView>("set_config", { patch }));
  }, []);

  const saveToken = useCallback(async (token: string) => {
    setConfig(await invoke<AppConfigView>("save_token", { token }));
  }, []);

  const deleteToken = useCallback(async () => {
    setConfig(await invoke<AppConfigView>("delete_token"));
  }, []);

  const closePanel = useCallback(() => {
    void invoke("hide_window").catch(console.error);
  }, []);

  const requestAccessibility = useCallback(async () => {
    const trusted = await invoke<boolean>("accessibility_status", { prompt: true });
    setAccessibilityTrusted(trusted);
    return trusted;
  }, []);

  const openAccessibilitySettings = useCallback(() => {
    void invoke("open_accessibility_settings").catch(console.error);
  }, []);

  return (
    <AsrContext.Provider
      value={{
        status,
        config,
        start,
        stop,
        saveConfig,
        saveToken,
        deleteToken,
        closePanel,
        accessibilityTrusted,
        requestAccessibility,
        openAccessibilitySettings,
      }}
    >
      {children}
    </AsrContext.Provider>
  );
}

export function useAsrClient(): AsrClient {
  const ctx = useContext(AsrContext);
  if (!ctx) throw new Error("useAsrClient must be used inside AsrProvider");
  return ctx;
}
