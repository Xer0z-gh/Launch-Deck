/**
 * The morning glance: one row answering the three questions worth asking
 * before touching anything — what is running, what died while I was away,
 * and which of my deployed/external surfaces are down.
 *
 * Every item is a control, not a caption: a death selects its project and
 * opens the log that shows it, a down surface selects its row, the running
 * count filters to the running collection. The strip earns its pixels only
 * when it has something to say; a quiet morning still states the facts
 * ("all quiet") in one short line rather than hiding, because "the strip
 * checked and found nothing" and "the strip did not check" must not look
 * the same.
 */
import { useQueries, useQuery } from "@tanstack/react-query";
import { CircleAlert, WifiOff } from "lucide-react";

import { cn } from "@/lib/cn";
import { formatRelative } from "@/lib/format";
import { api, isLive, type ProjectDto } from "@/lib/ipc";
import { useRuntime } from "@/state/runtime";
import { useUi } from "@/state/ui";

export function TodayStrip({ projects }: { projects: ProjectDto[] }) {
  const states = useRuntime((s) => s.states);
  const select = useUi((s) => s.select);
  const openLogs = useUi((s) => s.openLogs);
  const setCollection = useUi((s) => s.setCollection);

  const failures = useQuery({
    queryKey: ["overnight"],
    queryFn: () => api.overnightReport(),
    refetchInterval: 5 * 60_000,
    refetchOnWindowFocus: true,
  });

  // Same query keys and cadence as the row chips, so the strip and the chips
  // share one cache and one probe per project per minute — never two.
  const wired = projects.filter((p) => p.surface !== null);
  const surfaceReadings = useQueries({
    queries: wired.map((p) => ({
      queryKey: ["surface", p.id],
      queryFn: () => api.surfaceStatus(p.id),
      refetchInterval: 60_000,
      refetchOnWindowFocus: true,
      staleTime: 55_000,
    })),
  });

  const byId = new Map(projects.map((p) => [p.id, p]));
  const running = projects.filter((p) => isLive(states[p.id]));

  // A surface is "down" only on a definite negative reading: a transport
  // failure on a configured URL, or a closed configured port. A log-only
  // surface has no down state, and no reading yet is not a verdict.
  const down = wired.filter((p, i) => {
    const r = surfaceReadings[i]?.data;
    if (!r) return false;
    if (p.surface?.url) return r.httpError !== null;
    if (p.surface?.probePort != null) return r.portOpen === false;
    return false;
  });

  const died = (failures.data ?? []).filter((r) => byId.has(r.projectId));
  const quiet = died.length === 0 && down.length === 0;

  return (
    <section
      aria-label="Today"
      className="flex min-h-10 items-center gap-3 overflow-x-auto border-b border-rule bg-field px-4 py-1.5"
    >
      <span className="shrink-0 text-[13px] font-semibold tracking-[-0.078px] text-ink/60">
        Today
      </span>

      <button
        type="button"
        onClick={() => setCollection({ kind: "running" })}
        className={cn(
          "shrink-0 rounded-capsule px-2 py-1 text-[13px] tnum transition-colors",
          "hover:bg-fill-4",
          running.length > 0 ? "text-ink/85" : "text-ink/60",
        )}
      >
        <span
          aria-hidden
          className={cn(
            "mr-1.5 inline-block h-1.5 w-1.5 rounded-full align-middle",
            running.length > 0 ? "bg-accent" : "bg-ink/30",
          )}
        />
        {running.length > 0 ? `${running.length} running` : "none running"}
      </button>

      {died.map((r) => {
        const p = byId.get(r.projectId);
        if (!p) return null;
        return (
          <button
            key={r.runId}
            type="button"
            onClick={() => {
              select(p.id);
              openLogs(p.id);
            }}
            className={cn(
              "flex shrink-0 items-center gap-1.5 rounded-capsule px-2 py-1",
              "text-[13px] tnum text-signal transition-colors hover:bg-signal/10",
            )}
          >
            <CircleAlert size={12} aria-hidden />
            {/* When it DIED, not when it started -- a two-day-old server
                that fell over an hour ago died an hour ago. */}
            {p.name} died {formatRelative(r.finishedAt ?? r.startedAt)}
            {r.exitCode !== null ? ` · exit ${r.exitCode}` : ""}
          </button>
        );
      })}

      {down.map((p) => (
        <button
          key={p.id}
          type="button"
          onClick={() => select(p.id)}
          className={cn(
            "flex shrink-0 items-center gap-1.5 rounded-capsule px-2 py-1",
            "text-[13px] text-signal transition-colors hover:bg-signal/10",
          )}
        >
          <WifiOff size={12} aria-hidden />
          {p.name} down
        </button>
      ))}

      {quiet && !failures.isLoading && (
        <span className="shrink-0 text-[13px] text-ink/60">
          all quiet · {projects.filter((p) => !p.archived).length} projects ·{" "}
          {wired.length} watched
        </span>
      )}
    </section>
  );
}
