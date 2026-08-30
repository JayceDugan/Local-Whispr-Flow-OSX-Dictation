import { LoaderCircle, Mic } from "lucide-react";
import { useEffect, useState } from "react";
import { useAsrClient } from "@/hooks/useAsrClient";
import { cn } from "@/lib/utils";

/**
 * Big record button + local seconds ticker. Dumb: state comes from the
 * status snapshot; clicks call command wrappers only.
 */
export function RecordingIndicator() {
  const { status, start, stop } = useAsrClient();
  const phase = status?.phase ?? "idle";
  const recording = phase === "recording";
  const processing = phase === "processing";
  const disabled = phase === "serverStarting" || status == null;

  // Local tick from the moment recording began — avoids per-second IPC.
  const [seconds, setSeconds] = useState(0);
  useEffect(() => {
    if (!recording) {
      setSeconds(0);
      return;
    }
    const startedAt = Date.now();
    const id = window.setInterval(
      () => setSeconds(Math.floor((Date.now() - startedAt) / 1000)),
      500,
    );
    return () => window.clearInterval(id);
  }, [recording]);

  const toggle = () => {
    if (processing) return;
    void (recording ? stop() : start()).catch(() => {
      /* failures arrive as an asr://status error phase */
    });
  };

  return (
    <div className="flex flex-col items-center gap-3">
      <button
        type="button"
        onClick={toggle}
        disabled={disabled || processing}
        aria-label={recording ? "Stop recording" : "Start recording"}
        className={cn(
          "flex size-24 items-center justify-center rounded-full transition-all",
          "focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50",
          disabled && "cursor-not-allowed opacity-50",
          processing && "cursor-wait",
          recording
            ? "bg-destructive text-white shadow-lg shadow-destructive/30"
            : "bg-primary text-primary-foreground hover:bg-primary/90 active:scale-95",
        )}
      >
        {processing ? (
          <LoaderCircle className="size-9 animate-spin" />
        ) : recording ? (
          <span className="size-8 rounded-sm bg-white" />
        ) : (
          <Mic className="size-9" />
        )}
      </button>
      <p
        className={cn(
          "text-sm tabular-nums",
          recording ? "font-medium text-destructive" : "text-muted-foreground",
        )}
      >
        {processing
          ? "Transcribing…"
          : recording
            ? `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")} — press hotkey to stop`
            : disabled
              ? "Waiting for server"
              : "Press hotkey or click to dictate"}
      </p>
    </div>
  );
}
