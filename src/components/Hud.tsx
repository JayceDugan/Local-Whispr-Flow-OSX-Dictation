import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, LoaderCircle, Mic, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { AsrStatus } from "@/lib/types";
import { cn } from "@/lib/utils";
type HudState = "listening" | "transcribing" | "done" | "failed";

/**
 * Floating click-through pill (window label "hud"). Rust shows/hides the
 * window and emits `asr://hud`; this only renders the current state.
 */
export function Hud() {
  const [state, setState] = useState<HudState>("listening");

  useEffect(() => {
    let cancelled = false;
    void invoke<AsrStatus>("get_status")
      .then((s) => {
        if (cancelled) return;
        // Seed from phase in case the window was shown before `listen` attached.
        if (s.phase === "recording") setState("listening");
        else if (s.phase === "processing") setState("transcribing");
      })
      .catch(console.error);
    listen<HudState>("asr://hud", (e) => setState(e.payload))
      .then((un) => (cancelled ? un() : undefined))
      .catch(console.error);
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div className="flex h-screen w-screen items-center justify-center overflow-hidden bg-transparent select-none">
      <div
        className={cn(
          "flex items-center gap-2.5 rounded-full border border-white/10 px-4 py-2.5",
          "bg-neutral-900/85 text-neutral-100 shadow-xl backdrop-blur-md",
          "animate-in fade-in zoom-in-95 duration-150",
        )}
      >
        {state === "listening" && (
          <>
            <span className="relative flex size-3">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-red-500 opacity-60" />
              <span className="relative inline-flex size-3 rounded-full bg-red-500" />
            </span>
            <Mic className="size-4" />
            <span className="text-sm font-medium">Listening…</span>
          </>
        )}
        {state === "transcribing" && (
          <>
            <LoaderCircle className="size-4 animate-spin" />
            <span className="text-sm font-medium">Transcribing…</span>
          </>
        )}
        {state === "done" && (
          <>
            <span className="flex size-5 items-center justify-center rounded-full bg-emerald-500">
              <Check className="size-3.5 text-white" strokeWidth={3} />
            </span>
            <span className="text-sm font-medium">Added to clipboard</span>
          </>
        )}
        {state === "failed" && (
          <>
            <span className="flex size-5 items-center justify-center rounded-full bg-destructive">
              <X className="size-3.5 text-white" strokeWidth={3} />
            </span>
            <span className="text-sm font-medium">Something failed — see panel</span>
          </>
        )}
      </div>
    </div>
  );
}
