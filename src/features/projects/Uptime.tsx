import { formatUptime } from "@/lib/format";
import type { RunStateDto } from "@/lib/ipc";

import { useNow } from "./useNow";

/** Live uptime readout. Rendered only for running projects. */
export function Uptime({ state }: { state: RunStateDto }) {
  const now = useNow(1000);
  if (state.tag !== "running" || !state.startedAt) return null;
  const seconds = (now - Date.parse(state.startedAt)) / 1000;
  return <span className="tnum text-xs text-ink/60">{formatUptime(seconds)}</span>;
}
