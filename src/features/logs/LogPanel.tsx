import { save as pickSavePath } from "@tauri-apps/plugin-dialog";
import {
  ArrowDownToLine,
  ClipboardCopy,
  Download,
  Play,
  RotateCcw,
  Square,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";

import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Tooltip } from "@/components/ui/Tooltip";
import { parseAnsi } from "@/lib/ansi";
import { formatLogTime } from "@/lib/format";
import {
  api,
  asIpcError,
  canStart,
  canStop,
  type LogLineDto,
  type ProjectDto,
} from "@/lib/ipc";
import { cn } from "@/lib/cn";
import { useVirtual } from "@/hooks/useVirtual";
import { useRuntime } from "@/state/runtime";
import { useUi } from "@/state/ui";

import { StatusBadge } from "@/features/projects/StatusBadge";
import {
  useRestartProject,
  useStartProject,
  useStopProject,
} from "@/features/projects/useProjects";

const ROW_HEIGHT = 20;

type StreamFilter = "all" | "stdout" | "stderr" | "deck";

/** Slide-over log viewer: live console, filters, search, export. */
export function LogPanel({ project, open }: { project: ProjectDto; open: boolean }) {
  const close = useUi((s) => s.openLogs);
  const toast = useUi((s) => s.toast);
  const state = useRuntime((s) => s.states[project.id]);
  const buffer = useRuntime((s) => s.logs[project.id]);
  const replaceLogs = useRuntime((s) => s.replaceLogs);
  const seedMetrics = useRuntime((s) => s.seedMetrics);

  const start = useStartProject();
  const stop = useStopProject();
  const restart = useRestartProject();

  const [filter, setFilter] = useState<StreamFilter>("all");
  const [search, setSearch] = useState("");
  const [follow, setFollow] = useState(true);

  // Esc closes this panel and consumes the press, so the DetailBar under it
  // does not also clear the selection -- one layer per press, like the
  // studio and the dialogs.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      close(null);
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [close]);

  // Backfill the ring-buffer tail on open; live batches append via events.
  useEffect(() => {
    let cancelled = false;
    void api
      .getLogs(project.id)
      .then((logs) => {
        if (cancelled) return;
        const lines = logs.lines;
        replaceLogs(project.id, {
          lines,
          lastSeq: lines.length > 0 ? (lines[lines.length - 1]?.seq ?? -1) : -1,
          dropped: logs.dropped,
        });
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [project.id, replaceLogs]);

  // Backfill the sparkline history so a panel opened mid-run shows the trend
  // rather than starting from a single point.
  useEffect(() => {
    let cancelled = false;
    void api
      .projectMetrics(project.id)
      .then((samples) => {
        if (!cancelled && samples.length > 0) seedMetrics(project.id, samples);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [project.id, seedMetrics]);

  const allLines = useMemo(() => buffer?.lines ?? [], [buffer]);
  const needle = search.trim().toLowerCase();
  const lines = useMemo(() => {
    let out = allLines;
    if (filter !== "all") out = out.filter((l) => l.stream === filter);
    if (needle.length > 0) {
      out = out.filter((l) => l.text.toLowerCase().includes(needle));
    }
    return out;
  }, [allLines, filter, needle]);

  const containerRef = useRef<HTMLDivElement>(null);
  const virtual = useVirtual(lines.length, ROW_HEIGHT);

  // Follow mode: pin to the bottom as lines arrive; a wheel-up unpins.
  useEffect(() => {
    if (follow && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [lines.length, follow]);

  const onScroll = useCallback(
    (e: React.UIEvent<HTMLDivElement>) => {
      virtual.onScroll(e);
      const el = e.currentTarget;
      const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < ROW_HEIGHT * 2;
      setFollow(atBottom);
    },
    [virtual],
  );

  const copyVisible = () => {
    const text = lines.map((l) => l.text).join("\n");
    void navigator.clipboard
      .writeText(text)
      .then(() => toast("info", `Copied ${lines.length} lines`))
      .catch(() => toast("error", "Clipboard unavailable"));
  };

  const exportLog = async () => {
    const dest = await pickSavePath({
      title: "Export log",
      defaultPath: `${project.name}.log`,
    });
    if (typeof dest !== "string") return;
    try {
      await api.exportLogs(project.id, dest);
      toast("info", "Log exported");
    } catch (e) {
      toast("error", asIpcError(e).message);
    }
  };

  const filters: StreamFilter[] = ["all", "stdout", "stderr", "deck"];

  return (
    // Width is bounded rather than a raw viewport fraction: 45vw on a 1240px
    // window left the project list ~470px, below what its always-visible
    // columns need, so they collapsed on top of each other. A floor and a
    // ceiling keep both panes usable at any window size.
    <aside
      aria-label={`Logs for ${project.name}`}
      // Width animates rather than x-position: sliding the panel in *over* the
      // list would make the columns look like they were being shoved off the
      // edge -- the exact impression the container-query work exists to
      // prevent. Growing the panel lets the list reflow alongside it.
      //
      // Passed as custom properties, not as inline `width`/`min-width`: an
      // inline declaration outranks any class rule, so it would pin the panel
      // open and the exit would never move it. Custom properties are consumed
      // by `.panel-enter.is-open`, which the closed state can still override.
      style={{ "--panel-w": "40%", "--panel-min": "360px" } as CSSProperties}
      className={cn(
        "panel-enter flex h-full max-w-[880px] shrink-0 flex-col overflow-hidden border-l border-rule bg-panel",
        open && "is-open",
      )}
    >
      <header className="flex items-center gap-2 border-b border-rule px-4 py-2.5">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2.5">
            <h2 className="truncate text-[13px] font-semibold text-ink">
              {project.name}
            </h2>
            <StatusBadge state={state} />
          </div>
          {project.runCommand && (
            <p className="truncate font-mono text-[11px] text-ink/35">
              {project.runCommand}
            </p>
          )}
        </div>
        {canStop(state) ? (
          <Tooltip label="Stop">
            <Button size="icon" onClick={() => stop.mutate({ id: project.id })} aria-label="Stop">
              <Square size={13} className="text-signal" />
            </Button>
          </Tooltip>
        ) : (
          <Tooltip label="Run">
            <Button
              size="icon"
              disabled={!canStart(state)}
              onClick={() => start.mutate({ id: project.id })}
              aria-label="Run"
            >
              <Play size={13} className="text-accent" />
            </Button>
          </Tooltip>
        )}
        <Tooltip label="Restart">
          <Button size="icon" onClick={() => restart.mutate({ id: project.id })} aria-label="Restart">
            <RotateCcw size={13} />
          </Button>
        </Tooltip>
        <Tooltip label="Copy visible lines">
          <Button size="icon" onClick={copyVisible} aria-label="Copy visible lines">
            <ClipboardCopy size={13} />
          </Button>
        </Tooltip>
        <Tooltip label="Export full log…">
          <Button size="icon" onClick={() => void exportLog()} aria-label="Export log">
            <Download size={13} />
          </Button>
        </Tooltip>
        <Button size="icon" onClick={() => close(null)} aria-label="Close log panel">
          <X size={14} />
        </Button>
      </header>

      <div className="flex items-center gap-2 border-b border-rule px-4 py-2">
        <div className="flex gap-1" role="group" aria-label="Stream filter">
          {filters.map((f) => (
            <button
              key={f}
              type="button"
              onClick={() => setFilter(f)}
              className={cn(
                "rounded-control px-2 py-1 text-[11px] transition-colors",
                filter === f
                  ? "bg-raised text-ink"
                  : "text-ink/45 hover:bg-raised/60 hover:text-ink/70",
              )}
            >
              {f}
            </button>
          ))}
        </div>
        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search…"
          aria-label="Search logs"
          className="h-7 flex-1 text-xs"
        />
      </div>

      {buffer && buffer.dropped > 0 && (
        <p className="border-b border-rule bg-field px-4 py-1 text-[11px] text-ink/40">
          {buffer.dropped.toLocaleString()} earlier lines not held in memory — export
          for the full log.
        </p>
      )}

      <div
        ref={(el) => {
          containerRef.current = el;
          if (el) virtual.setViewport(el.clientHeight);
        }}
        onScroll={onScroll}
        className="min-h-0 flex-1 overflow-auto bg-field font-mono text-xs leading-5"
      >
        {lines.length === 0 ? (
          <p className="px-4 py-6 text-center font-sans text-xs text-ink/40">
            {allLines.length === 0
              ? "No output yet. Press Run to launch this project."
              : "Nothing matches the current filter."}
          </p>
        ) : (
          <div style={{ height: virtual.totalHeight, position: "relative" }}>
            <div style={{ transform: `translateY(${virtual.offsetY}px)` }}>
              {lines.slice(virtual.start, virtual.end).map((line) => (
                <LogRow key={line.seq} line={line} />
              ))}
            </div>
          </div>
        )}
      </div>

      {!follow && lines.length > 0 && (
        <button
          type="button"
          onClick={() => {
            setFollow(true);
            if (containerRef.current) {
              containerRef.current.scrollTop = containerRef.current.scrollHeight;
            }
          }}
          className="flex items-center justify-center gap-1.5 border-t border-rule bg-raised px-4 py-1.5 text-[11px] text-accent hover:bg-overlay"
        >
          <ArrowDownToLine size={12} />
          Following paused — jump to latest
        </button>
      )}
    </aside>
  );
}

/** Heuristic error emphasis, mirroring `LogLine::looks_like_error`. */
function looksLikeError(text: string): boolean {
  const lower = text.trimStart().toLowerCase();
  return (
    lower.startsWith("error") ||
    lower.startsWith("err!") ||
    lower.startsWith("fatal") ||
    lower.startsWith("panic") ||
    lower.startsWith("exception") ||
    lower.startsWith("traceback") ||
    lower.startsWith("unhandled") ||
    lower.startsWith("uncaught") ||
    lower.startsWith("failed") ||
    lower.startsWith("[error")
  );
}

function LogRow({ line }: { line: LogLineDto }) {
  const spans = useMemo(() => parseAnsi(line.text), [line.text]);
  const isDeck = line.stream === "deck";
  const isError = !isDeck && looksLikeError(line.text);

  return (
    <div
      className={cn(
        "flex h-5 items-center gap-2 whitespace-pre px-4",
        isDeck && "italic text-ink/45",
        isError && "bg-signal/[0.07]",
      )}
    >
      <span className="tnum shrink-0 select-none text-[10px] text-ink/25">
        {formatLogTime(line.at)}
      </span>
      <span
        className={cn(
          "w-10 shrink-0 select-none text-[10px] uppercase",
          line.stream === "stderr" ? "text-signal/60" : "text-ink/25",
        )}
      >
        {line.stream}
      </span>
      <span className={cn("min-w-0", isError && "text-signal")}>
        {spans.map((span, i) => (
          <span
            // Spans have no identity beyond position within one immutable line.
            key={i}
            style={{
              color: span.color,
              backgroundColor: span.background,
              fontWeight: span.bold ? 600 : undefined,
              opacity: span.dim ? 0.6 : undefined,
              fontStyle: span.italic ? "italic" : undefined,
              textDecoration: span.underline ? "underline" : undefined,
            }}
          >
            {span.text}
          </span>
        ))}
      </span>
    </div>
  );
}
