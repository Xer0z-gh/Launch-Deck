import { ExternalLink, Play, RotateCcw, ScrollText, Square, SquareTerminal, X } from "lucide-react";
import { useEffect } from "react";

import { Button } from "@/components/ui/Button";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { formatRelative } from "@/lib/format";
import { formatBytes, formatCpu, formatPorts } from "@/lib/units";
import { api, canStart, canStop, type ProjectDto } from "@/lib/ipc";
import { useRuntime } from "@/state/runtime";
import { useUi } from "@/state/ui";

import { ProjectIcon } from "./ProjectIcon";
import { SurfaceChip } from "./SurfaceChip";
import { RowActions } from "./RowActions";
import { StatusBadge } from "./StatusBadge";
import { Uptime } from "./Uptime";
import { useRestartProject, useStartProject, useStopProject } from "./useProjects";

/**
 * The selection bar: pick a tile, get the full picture and a prominent
 * launch control — the LaunchBox select-then-play flow. Esc dismisses.
 */
export function DetailBar({ project, open }: { project: ProjectDto; open: boolean }) {
  const select = useUi((s) => s.select);
  const openLogs = useUi((s) => s.openLogs);
  const openStudio = useUi((s) => s.openStudio);
  const state = useRuntime((s) => s.states[project.id]);
  const samples = useRuntime((s) => s.metrics[project.id]);
  const latest = samples?.[samples.length - 1];

  const start = useStartProject();
  const stop = useStopProject();
  const restart = useRestartProject();

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      // Esc peels ONE layer per press. While a side panel is up, that press
      // belongs to the panel — reading the store (not props) keeps this
      // correct regardless of listener registration order.
      const { studioFor, logsFor } = useUi.getState();
      if (studioFor || logsFor) return;
      select(null);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [select]);

  const stoppable = canStop(state);

  return (
    <footer
      aria-label={`Selected: ${project.name}`}
      // Height, not slide: the bar is part of the column's flow, so it should
      // look like the layout made room for it rather than like something
      // arrived on top. `.bar-enter` transitions to `height: auto` via
      // `interpolate-size`, so the bar is sized by its content instead of
      // carrying a magic pixel number that breaks as soon as the content wraps.
      className={cn(
        "bar-enter @container flex shrink-0 items-center gap-4 overflow-hidden border-t border-rule bg-panel px-4 py-3 [box-shadow:var(--hairline-top)]",
        open && "is-open",
      )}
    >
      <ProjectIcon project={project} size={44} />

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2.5">
          <h2 className="truncate text-sm font-semibold text-ink">{project.name}</h2>
          <StatusBadge state={state} />
          {/* The detailed chip yields before the name does: in a narrow bar
              it was erasing the h2 and running under the Run button (both
              measured), and the row's own chip still carries the reading. */}
          <span className="hidden min-w-0 @min-[640px]:inline-flex">
            <SurfaceChip project={project} detailed />
          </span>
          {state?.tag === "running" && <Uptime state={state} />}
        </div>
        <p className="mt-0.5 flex items-center gap-2 text-[11px] text-ink/45">
          <span className="shrink-0">
            {project.kindLabel}
            {project.version ? ` · v${project.version}` : ""}
          </span>
          <span className="shrink-0 text-ink/25">·</span>
          <span className="truncate text-ink/35">{project.root}</span>
        </p>
        {project.runCommand && (
          <p className="mt-0.5 truncate font-mono text-[11px] text-ink/30">
            {project.runCommand}
          </p>
        )}
      </div>

      {latest ? (
        <dl className="hidden shrink-0 grid-cols-[auto_auto] gap-x-3 gap-y-0.5 text-[11px] @min-[720px]:grid">
          <dt className="text-ink/35">CPU</dt>
          <dd className="tnum whitespace-nowrap text-right text-ink/70">{formatCpu(latest.cpuPercent)}</dd>
          <dt className="text-ink/35">RAM</dt>
          <dd className="tnum whitespace-nowrap text-right text-ink/70">{formatBytes(latest.memoryBytes)}</dd>
          <dt className="text-ink/35">Ports</dt>
          <dd className="tnum whitespace-nowrap text-right text-ink/70">
            {formatPorts(latest.listeningPorts)}
          </dd>
          <dt className="text-ink/35">PID</dt>
          <dd className="tnum whitespace-nowrap text-right text-ink/70">
            {latest.pid}
            <span className="text-ink/35"> &middot; {latest.processCount} proc</span>
          </dd>
        </dl>
      ) : (
        <span className="tnum hidden shrink-0 text-[11px] text-ink/35 @min-[720px]:block">
          last launched {formatRelative(project.lastLaunchedAt)}
        </span>
      )}

      <div className="flex shrink-0 items-center gap-1.5">
        {stoppable ? (
          <Button variant="outline" size="md" onClick={() => stop.mutate({ id: project.id })}>
            <Square size={12} className="text-signal" />
            Stop
          </Button>
        ) : (
          <Button
            variant="primary"
            size="md"
            disabled={!canStart(state) || start.isPending}
            onClick={() => start.mutate({ id: project.id })}
          >
            <Play size={12} />
            Run
          </Button>
        )}
        <Tooltip label="Restart">
          <Button
            size="icon"
            aria-label="Restart"
            disabled={!stoppable && !canStart(state)}
            onClick={() => restart.mutate({ id: project.id })}
            className="hidden @min-[560px]:inline-flex"
          >
            <RotateCcw size={13} />
          </Button>
        </Tooltip>
        {project.surface?.url && (
          <Tooltip label={`Open ${project.surface.url}`}>
            <Button
              size="icon"
              aria-label={`Open ${project.name} in browser`}
              onClick={() => void api.openSurfaceUrl(project.id)}
            >
              <ExternalLink size={13} />
            </Button>
          </Tooltip>
        )}
        <Tooltip label="Logs">
          <Button
            size="icon"
            aria-label="Logs"
            onClick={() => openLogs(project.id)}
            className="hidden @min-[560px]:inline-flex"
          >
            <ScrollText size={13} />
          </Button>
        </Tooltip>
        <Tooltip label="Build prompt">
          <Button
            size="icon"
            aria-label={`Build prompt for ${project.name}`}
            onClick={() => openStudio(project.id)}
            className="hidden @min-[560px]:inline-flex"
          >
            <SquareTerminal size={13} />
          </Button>
        </Tooltip>
        <RowActions project={project} compact />
        <Button size="icon" aria-label="Clear selection" onClick={() => select(null)}>
          <X size={13} />
        </Button>
      </div>
    </footer>
  );
}
