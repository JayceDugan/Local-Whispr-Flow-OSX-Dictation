import { useAsrClient } from "@/hooks/useAsrClient";
import type { Health } from "@/lib/types";

/**
 * Convenience view over the health field of the status snapshot. Rust polls
 * /healthz and pushes changes; no polling happens in JS.
 */
export function useServerHealth(): {
  health: Health;
  online: boolean;
  starting: boolean;
} {
  const { status } = useAsrClient();
  const health: Health = status?.health ?? "online";
  return {
    health,
    online: health === "online",
    starting: status?.phase === "serverStarting",
  };
}
