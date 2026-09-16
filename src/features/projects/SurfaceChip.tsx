/**
 * The status-source reading: one glanceable fact about a surface the
 * supervisor does not own, wired to a real probe or absent.
 *
 * Renders nothing for projects with no status source configured — an unwired
 * project simply has no chip, and the chip never invents a reading (the
 * no-fake-data rule this re-scope is built on). When a URL is configured the
 * chip prefers the HTTP reading (status + latency, real units, tabular
 * numerals); a port-only config shows the port with its open/closed state.
 *
 * The chip is itself the refresh action: clicking re-probes now. Display-only
 * elements do not ship.
 */
import { useQuery } from "@tanstack/react-query";

import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { formatAge } from "@/lib/format";
import { api, type ProjectDto, type SurfaceStatusDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

/**
 * Polls a wired project's status source.
 *
 * Once a minute plus on window focus: the morning-open use case is "glance at
 * the true state", so returning to the window re-probes. Manual refresh via
 * the chip covers the impatient path. The backend schedules nothing.
 */
export function useSurfaceStatus(project: ProjectDto) {
  return useQuery({
    queryKey: ["surface", project.id],
    queryFn: () => api.surfaceStatus(project.id),
    enabled: project.surface !== null,
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
    staleTime: 55_000,
  });
}

/** One reading, compressed to a dot and a number. */
export function SurfaceChip({
  project,
  detailed = false,
  className,
}: {
  project: ProjectDto;
  /** DetailBar variant: adds the log-file age when one is configured. */
  detailed?: boolean;
  className?: string;
}) {
  const { data, isFetching, refetch } = useSurfaceStatus(project);
  const toast = useUi((s) => s.toast);
  if (!project.surface) return null;

  const { label, tone, title } = describe(project, data, isFetching);

  return (
    <Tooltip label={title}>
      <button
        type="button"
        // A real button, because the chip IS the manual re-probe. Propagation
        // stops on click AND keydown: without the keydown guard, Enter on the
        // chip bubbled to the row and shoved the log panel open too.
        onClick={(e) => {
          e.stopPropagation();
          // A manual re-probe announces its result — the reading otherwise
          // changes only inside aria-label, which nothing reads back. The
          // 60 s background poll stays silent on purpose.
          void refetch().then((r) => {
            const fresh = describe(project, r.data, false);
            toast("info", `${project.name} — ${fresh.label}`);
          });
        }}
        onKeyDown={(e) => e.stopPropagation()}
        aria-label={`Re-check status source for ${project.name}: ${label}`}
        className={cn(
          "flex min-h-6 shrink-0 items-center gap-1.5 rounded-control px-1.5 py-1",
          "text-[11px] leading-none tnum transition-colors duration-[120ms]",
          "hover:bg-raised/70",
          tone === "up" && "text-ink/70",
          tone === "down" && "text-signal",
          // /60, not /40: "log 65d" or "not checked" is the chip's WHOLE
          // reading, and 11px text below ~4.5:1 fails the contrast floor.
          tone === "dim" && "text-ink/60",
          className,
        )}
      >
        <span
          aria-hidden
          className={cn(
            "h-1.5 w-1.5 shrink-0 rounded-full",
            tone === "up" && "bg-accent",
            tone === "down" && "bg-signal",
            tone === "dim" && "bg-ink/30",
          )}
        />
        {label}
        {detailed && data?.logMtime && (
          <span className="text-ink/60">· log {formatAge(data.logMtime)}</span>
        )}
      </button>
    </Tooltip>
  );
}

/** Turns a reading into the chip's one line, honestly. */
function describe(
  project: ProjectDto,
  data: SurfaceStatusDto | undefined,
  isFetching: boolean,
): { label: string; tone: "up" | "down" | "dim"; title: string } {
  const cfg = project.surface;
  if (!data) {
    return {
      label: isFetching ? "checking" : "not checked",
      tone: "dim",
      title: "No reading yet — click to probe now",
    };
  }

  const checked = `checked ${new Date(data.checkedAt).toLocaleTimeString()} — click to re-check`;

  // HTTP is the richest signal, so a configured URL speaks first.
  if (cfg?.url) {
    if (data.httpStatus !== null) {
      const ok = data.httpStatus >= 200 && data.httpStatus < 400;
      return {
        // The real status code and real milliseconds. `200 · 187 ms` answers
        // "alive, and how it feels" in one glance; a non-2xx shows its code
        // rather than collapsing into "down" (a 403 is a live server).
        label: `${data.httpStatus} · ${data.httpMs} ms`,
        tone: ok ? "up" : "down",
        title: `${cfg.url} answered ${data.httpStatus} in ${data.httpMs} ms · ${checked}`,
      };
    }
    if (data.httpError !== null) {
      return {
        label: "down",
        tone: "down",
        title: `${cfg.url}: ${data.httpError} · ${checked}`,
      };
    }
  }

  if (cfg?.probePort !== null && cfg?.probePort !== undefined) {
    return data.portOpen
      ? {
          label: `:${cfg.probePort} up`,
          tone: "up",
          title: `Something is answering on 127.0.0.1:${cfg.probePort} · ${checked}`,
        }
      : {
          label: `:${cfg.probePort} closed`,
          tone: "down",
          title: `Nothing is listening on 127.0.0.1:${cfg.probePort} · ${checked}`,
        };
  }

  // Only a log file configured: its age is the whole signal.
  if (data.logMtime !== null) {
    return {
      label: `log ${formatAge(data.logMtime)}`,
      tone: "dim",
      title: `Log last written ${new Date(data.logMtime).toLocaleString()} · ${checked}`,
    };
  }
  return { label: "no signal", tone: "dim", title: checked };
}

