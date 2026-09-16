/**
 * Live runtime state: run states and log buffers, fed by backend events.
 *
 * Event wiring lives in `initRuntimeEvents`, called ONCE from main.tsx before
 * React renders — listeners are process-level singletons, deliberately outside
 * any component lifecycle so StrictMode double-effects cannot double-register
 * them.
 */

import { create } from "zustand";

import {
  api,
  onLogsEvent,
  onMetricsEvent,
  onRunFinished,
  onStateEvent,
  type LogLineDto,
  type MetricsDto,
  type RunStateDto,
} from "@/lib/ipc";

/** Mirror of the backend ring capacity: the UI never holds more per project. */
const LOG_CAP = 5000;

/** Samples retained per project for the sparkline. Mirrors the backend ring. */
const METRICS_CAP = 120;

export interface LogBuffer {
  lines: LogLineDto[];
  /** Highest sequence number seen, for gap detection and resume. */
  lastSeq: number;
  /** Lines known to have been evicted before what we hold. */
  dropped: number;
}

interface RuntimeState {
  states: Record<string, RunStateDto>;
  logs: Record<string, LogBuffer>;
  /** Live resource samples per project, oldest first. */
  metrics: Record<string, MetricsDto[]>;
  applyState: (projectId: string, state: RunStateDto) => void;
  applyMetrics: (samples: MetricsDto[]) => void;
  seedMetrics: (projectId: string, samples: MetricsDto[]) => void;
  appendLogs: (projectId: string, lines: LogLineDto[]) => void;
  replaceLogs: (projectId: string, buffer: LogBuffer) => void;
  clearProject: (projectId: string) => void;
}

export const useRuntime = create<RuntimeState>((set) => ({
  states: {},
  logs: {},
  metrics: {},

  applyState: (projectId, state) =>
    set((prev) => ({ states: { ...prev.states, [projectId]: state } })),

  applyMetrics: (samples) =>
    set((prev) => {
      // One event carries a sample per running project, so fold the whole
      // batch into a single state update rather than one per project.
      const metrics = { ...prev.metrics };
      for (const sample of samples) {
        const existing = metrics[sample.projectId] ?? [];
        const next = existing.concat(sample);
        metrics[sample.projectId] =
          next.length > METRICS_CAP ? next.slice(next.length - METRICS_CAP) : next;
      }
      return { metrics };
    }),

  seedMetrics: (projectId, samples) =>
    set((prev) => ({ metrics: { ...prev.metrics, [projectId]: samples } })),

  appendLogs: (projectId, incoming) =>
    set((prev) => {
      const current = prev.logs[projectId] ?? { lines: [], lastSeq: -1, dropped: 0 };
      // A new run restarts sequence numbering at 0; detect it and reset the
      // buffer instead of appending a second run onto the first.
      const first = incoming[0];
      const isNewRun = first !== undefined && first.seq < current.lastSeq - LOG_CAP;
      const base = isNewRun ? { lines: [], lastSeq: -1, dropped: 0 } : current;

      const fresh = incoming.filter((l) => l.seq > base.lastSeq);
      if (fresh.length === 0) return prev;

      let lines = base.lines.concat(fresh);
      let dropped = base.dropped;
      if (lines.length > LOG_CAP) {
        dropped += lines.length - LOG_CAP;
        lines = lines.slice(lines.length - LOG_CAP);
      }
      const lastSeq = lines.length > 0 ? (lines[lines.length - 1]?.seq ?? -1) : -1;
      return {
        logs: { ...prev.logs, [projectId]: { lines, lastSeq, dropped } },
      };
    }),

  replaceLogs: (projectId, buffer) =>
    set((prev) => ({ logs: { ...prev.logs, [projectId]: buffer } })),

  clearProject: (projectId) =>
    set((prev) => {
      const states = { ...prev.states };
      const logs = { ...prev.logs };
      const metrics = { ...prev.metrics };
      delete states[projectId];
      delete logs[projectId];
      delete metrics[projectId];
      return { states, logs, metrics };
    }),
}));

/** Wires backend events into the store and seeds current state. Call once. */
export async function initRuntimeEvents(onFinished: () => void): Promise<void> {
  await onStateEvent((e) => {
    useRuntime.getState().applyState(e.projectId, e.state);
  });
  await onLogsEvent((e) => {
    useRuntime.getState().appendLogs(e.projectId, e.lines);
  });
  await onMetricsEvent((e) => {
    useRuntime.getState().applyMetrics(e.samples);
  });
  await onRunFinished((e) => {
    // The run is over, so its samples describe a process that no longer
    // exists; keeping them would leave a stale CPU figure on screen.
    useRuntime.setState((prev) => {
      const metrics = { ...prev.metrics };
      delete metrics[e.projectId];
      return { metrics };
    });
    onFinished();
  });

  // Seed with whatever is already running (relevant after a frontend reload).
  const seeded = await api.runStates();
  for (const entry of seeded) {
    useRuntime.getState().applyState(entry.projectId, entry.state);
  }
}
