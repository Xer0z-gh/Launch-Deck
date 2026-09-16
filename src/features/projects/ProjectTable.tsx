import { Pin, Star, TriangleAlert } from "lucide-react";
import { useRef, type KeyboardEvent as ReactKeyboardEvent } from "react";

import { cn } from "@/lib/cn";
import { formatRelative } from "@/lib/format";
import type { ProjectDto } from "@/lib/ipc";
import { formatBytes, formatCpu } from "@/lib/units";
import { useRuntime } from "@/state/runtime";
import { useUi } from "@/state/ui";

import { ProjectIcon } from "./ProjectIcon";
import { useSetupReport } from "./useSetupReport";
import { RowActions } from "./RowActions";
import { SurfaceChip } from "./SurfaceChip";
import { usePrewarm } from "./usePrewarm";
import { StatusBadge } from "./StatusBadge";

/**
 * The project library as an INSET GROUPED LIST.
 *
 * # Why this is not a table
 *
 * It was one, and that was the single largest thing making the app read as
 * Windows rather than Apple: a seven-column grid with a header row, six
 * values per row and five naked glyph buttons at the end. Apple does not
 * ship that shape as a primary screen anywhere -- Settings, Music, Mail,
 * Podcasts and Shortcuts are all the same object: a rounded card floating on
 * a grouped background, rows of one title, one supporting line, one trailing
 * value, and one accessory.
 *
 * The previous pass kept the grid and argued that iPadOS Files shows columns
 * in list view. That argument was wrong twice over: Files' column view is a
 * secondary presentation of a folder the user switched into, and this screen
 * is a list of APPS, which is the grouped-list case. Tanner rejected it on
 * sight, correctly.
 *
 * # What each row is allowed to carry
 *
 * Leading icon, title, one supporting line, one trailing value, one
 * accessory. Everything the grid used to spread across six columns is either
 * folded into the supporting line (kind, version, last run), shown only when
 * it is live and therefore interesting (CPU, memory), or moved into the row's
 * context menu, where it already existed.
 *
 * # Sorting
 *
 * The sort control moved to the toolbar, because a grouped list has no
 * header row to click. That is what Files, Photos and Mail do, and the
 * ordering itself is unchanged -- `sort.ts` still decides it.
 */
export function ProjectTable({ projects }: { projects: ProjectDto[] }) {
  const openLogs = useUi((s) => s.openLogs);
  const listRef = useRef<HTMLUListElement>(null);
  // One prewarmer for the whole list rather than one per row -- there is
  // nothing per-row about it.
  const prewarm = usePrewarm();

  const onKeyDown = (e: ReactKeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const rows = Array.from(
      listRef.current?.querySelectorAll<HTMLLIElement>("li[tabindex]") ?? [],
    );
    const current = rows.indexOf(document.activeElement as HTMLLIElement);
    const next = e.key === "ArrowDown" ? current + 1 : current - 1;
    rows[Math.max(0, Math.min(rows.length - 1, next))]?.focus();
  };

  return (
    // The inset: a grouped list never spans the window edge to edge. The page
    // margin is what makes the card read as a card.
    // Wide enough that a large window does not leave the card marooned in the
    // middle of an empty field -- the previous 900px cap produced a ~400px
    // dead gutter beside the sidebar. Still capped, because a list row that
    // spans 1900px puts its title and its controls on opposite horizons.
    <div className="@container mx-auto w-full max-w-[1180px] px-1">
      {/* A grouped-list header: sentence case, secondary label, and it states
          the count so the list does not need a column to. */}
      <h2 className="px-4 pb-2 text-[13px] font-semibold tracking-[-0.078px] text-ink/60">
        {projects.length} {projects.length === 1 ? "project" : "projects"}
      </h2>
      <ul
        ref={listRef}
        onKeyDown={onKeyDown}
        className="overflow-hidden rounded-card bg-panel"
      >
        {projects.map((project) => (
          <Row
            key={project.id}
            project={project}
            onOpen={() => openLogs(project.id)}
            prewarm={prewarm}
          />
        ))}
      </ul>
    </div>
  );
}

function Row({
  project,
  onOpen,
  prewarm,
}: {
  project: ProjectDto;
  onOpen: () => void;
  prewarm: ReturnType<typeof usePrewarm>;
}) {
  const state = useRuntime((s) => s.states[project.id]);
  const samples = useRuntime((s) => s.metrics[project.id]);
  const setup = useSetupReport(project.id);
  const select = useUi((s) => s.select);
  const selected = useUi((s) => s.selectedId === project.id);
  const latest = samples?.[samples.length - 1];
  const live =
    state?.tag === "running" || state?.tag === "starting" || state?.tag === "restarting";

  // The supporting line. One line, in priority order: what it is, then when
  // it last ran. The path is gone from the row -- it is in the detail bar and
  // the context menu, and a full Windows path is the least Apple thing a list
  // row could carry.
  const support = [
    project.kindLabel,
    project.version ? `v${project.version}` : null,
    live ? null : `last run ${formatRelative(project.lastLaunchedAt)}`,
  ]
    .filter(Boolean)
    .join(" · ");

  /**
   * A reason this will not start, shown INSTEAD of the supporting line.
   *
   * Instead, not beside: the supporting line says what a project is, and that
   * is the least useful fact about one that cannot run. Real history showed
   * Transcriber failing fourteen times on a missing Python module with nothing
   * on the row to say so -- the state resets when the app restarts, and the
   * only evidence lived in a log panel nobody opens before clicking Run.
   *
   * A launch that is merely known to have failed LAST time is reported as
   * exactly that, and only when nothing static explains it, because "it broke
   * before" is weaker evidence than "the folder is gone".
   */
  const trouble = live
    ? null
    : (setup?.reason ??
      (setup?.lastFailedExit !== null && setup?.lastFailedExit !== undefined
        ? `Last run failed · exit ${setup.lastFailedExit}`
        : null));

  return (
    <li
      // `data-project-row` is the stable hook the verify suites address. It
      // survives presentation changes; `tbody tr` did not.
      data-project-row
      data-live={live || undefined}
      data-selected={selected || undefined}
      tabIndex={0}
      onClick={() => {
        select(project.id);
        // Selection is deliberate and usually precedes Run -- see `usePrewarm`
        // for why this is not also done on hover.
        prewarm.now(project.id);
      }}
      onDoubleClick={onOpen}
      onKeyDown={(e) => {
        if (e.key === "Enter") onOpen();
      }}
      // No inline style here, deliberately. The row's position in the entrance
      // cascade comes from `:nth-child` in `tokens.css`: an inline custom
      // property makes every row unique, which locks all of them out of
      // Blink's computed-style sharing and cost 15.4 ms -- 47% -- of every
      // full style recalc. Guarded by `tools/verify/style-cost.mjs`.
      className={cn(
        // 60px is Apple's two-line row. The leading padding is 16, and the
        // separator below starts at 64 so it aligns with the TITLE, not the
        // icon -- the inset separator is one of the most recognisable
        // details of an iOS list and the old full-bleed rule had none.
        "row-in relative flex h-[60px] cursor-default items-center gap-3 pl-4 pr-2",
        "after:absolute after:bottom-0 after:left-[57px] after:right-0 after:h-px",
        "after:bg-rule last:after:hidden",
        "transition-colors duration-[140ms] ease-[var(--ease-standard)]",
        // iOS highlights with system fills, and marks selection with the
        // tint at low opacity rather than a lighter slab.
        "hover:bg-fill-4 data-[selected]:bg-accent/15",
        "focus:outline-none focus-visible:[box-shadow:inset_0_0_0_2px_var(--color-accent)]",
        // Skip rendering rows that are scrolled out of view. The browser-native
        // alternative to a windowing library, and enough for a list of
        // fixed-height rows; `contain-intrinsic-size` keeps the scrollbar
        // honest for rows it has not measured.
        "[content-visibility:auto] [contain-intrinsic-size:auto_60px]",
      )}
    >
      {/* 29pt icon: Apple's list-row icon size, and big enough that the mark
          is legible rather than decorative. */}
      <ProjectIcon project={project} size={29} className="shrink-0" />

      <div className="min-w-0 flex-1">
        <div className="flex min-w-0 items-center gap-1.5">
          {project.pinned && <Pin size={11} className="shrink-0 text-ink/45" />}
          {project.favorite && <Star size={11} className="shrink-0 text-warn" />}
          <span className="truncate text-[17px] tracking-[-0.408px] text-ink-strong">
            {project.name}
          </span>
          <SurfaceChip project={project} />
        </div>
        {trouble ? (
          <p className="flex min-w-0 items-center gap-1.5 text-[15px] tracking-[-0.24px] text-signal">
            <TriangleAlert size={12} aria-hidden className="shrink-0" />
            <span className="truncate">{trouble}</span>
          </p>
        ) : (
          <p className="truncate text-[15px] tracking-[-0.24px] text-ink/70">{support}</p>
        )}
      </div>

      {/* Trailing value: live resources while there is a process to measure,
          the run state otherwise. One value, never six columns of them. */}
      {live && latest ? (
        <span className="tnum hidden shrink-0 whitespace-nowrap text-[15px] text-ink/60 @min-[560px]:block">
          {formatCpu(latest.cpuPercent)} · {formatBytes(latest.memoryBytes)}
        </span>
      ) : (
        <StatusBadge state={state} labelClassName="hidden @min-[420px]:inline" />
      )}

      {/* One primary action and one accessory, Apple-style. */}
      <RowActions project={project} minimal />
    </li>
  );
}
