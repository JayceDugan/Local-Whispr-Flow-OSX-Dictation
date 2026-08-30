import { CircleDot, TriangleAlert } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { useAsrClient } from "@/hooks/useAsrClient";
import { ERROR_HEADLINE } from "@/lib/types";

/**
 * Contextual banner: server-starting notice, typed error card, or the last
 * transcript with latency / uncleaned chips. Pure presentation.
 */
export function StatusBanner() {
  const { status } = useAsrClient();
  if (!status) return null;

  if (status.phase === "serverStarting") {
    return (
      <div className="flex items-start gap-2 rounded-lg border bg-muted/50 p-3 text-sm">
        <CircleDot className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
        <p className="text-muted-foreground">
          ASR server is not reachable yet. A cold model load can take a few
          minutes — dictation unlocks automatically when it comes up.
        </p>
      </div>
    );
  }

  if (status.phase === "error" && status.error) {
    return (
      <div className="flex items-start gap-2 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm">
        <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
        <div className="space-y-0.5">
          <p className="font-medium">
            {ERROR_HEADLINE[status.error.kind]}
          </p>
          <p className="text-muted-foreground">{status.error.message}</p>
        </div>
      </div>
    );
  }

  const r = status.lastResult;
  if (!r) return null;

  return (
    <div className="space-y-2 rounded-lg border bg-card p-3">
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
          Last dictation
        </p>
        <div className="flex items-center gap-1.5">
          {!r.cleanupApplied && (
            <Badge variant="secondary" title="Server returned uncleaned text; injected as spoken">
              uncleaned
            </Badge>
          )}
          <Badge
            variant={r.slow ? "outline" : "secondary"}
            title={r.slow ? "Slow path — likely cold engine or network" : undefined}
          >
            {(r.latencyMs / 1000).toFixed(2)}s
          </Badge>
        </div>
      </div>
      <p className="max-h-28 overflow-y-auto text-sm leading-relaxed break-words">
        {r.text}
      </p>
      {r.warning && (
        <p className="text-xs text-muted-foreground">{r.warning}</p>
      )}
    </div>
  );
}
