/**
 * Storage: what the library actually costs on disk, and where.
 *
 * # Why this is measured one project at a time
 *
 * A single sweep of every root took over five minutes on a cold cache in this
 * workspace, and there is no honest progress bar for a walk whose size is
 * unknown until it finishes. So each row asks for its own measurement and the
 * list fills in as answers land -- useful from the first result rather than
 * blank until the last. The query cache keeps the answers for the session, and
 * the Re-measure button is how you ask again after a build.
 *
 * # Why the bar colour means a size, not a rank
 *
 * The bar LENGTH is relative to the largest project, because that is what
 * makes a bar chart readable. The bar COLOUR is absolute: the same 30 GB is
 * the same red whether or not something bigger sits above it. A palette that
 * rescaled with the maximum would turn the whole screen red the moment the
 * biggest project was archived, which is the opposite of informative.
 *
 * Colour is never the only carrier: every row states its size in tabular
 * numerals and the bar encodes it again as length, so the hierarchy survives
 * greyscale and a screen reader hears the band name in the row's label.
 */
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowDownWideNarrow, ChevronRight, RefreshCw } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { api, type DirChildDto, type ProjectDto } from "@/lib/ipc";
import { formatBytes } from "@/lib/units";
import { useUi } from "@/state/ui";

import { ProjectIcon } from "@/features/projects/ProjectIcon";
import { Card, CardEmpty, SectionTitle } from "@/features/dashboard/primitives";

/**
 * Band thresholds, in the same 1024 base `formatBytes` renders.
 *
 * Matching the base matters: with decimal thresholds a project displayed as
 * "1.9 GB" would already be in the 2 GB band, and the boundary would look
 * arbitrary to anyone reading the number beside it.
 */
const GIB = 1024 ** 3;
const MIB = 1024 ** 2;

/** Colour and name for an absolute size. */
function band(bytes: number): { bar: string; text: string; label: string } {
  // 10 GB in a source project is almost always a build directory nobody meant
  // to keep, so the top band gets the colour this app reserves for problems.
  if (bytes >= 10 * GIB) return { bar: "bg-signal", text: "text-signal", label: "very large" };
  if (bytes >= 2 * GIB) return { bar: "bg-warn", text: "text-warn", label: "large" };
  if (bytes >= 200 * MIB) return { bar: "bg-accent", text: "text-ink/80", label: "moderate" };
  return { bar: "bg-ink/30", text: "text-ink/70", label: "small" };
}

/**
 * Size for display.
 *
 * Two corrections over `formatBytes`, both because this screen means something
 * different by a small number than the rest of the app does:
 *
 * - It renders an em dash at zero, which is right where a value was never
 *   measured. Here zero IS the measurement, so it says "empty".
 * - It rounds sub-kilobyte files to "0 KB", which reads as nothing at all for
 *   a real 400-byte file sitting in a list of real files.
 */
function size(bytes: number): string {
  if (bytes === 0) return "empty";
  if (bytes < 1024) return "<1 KB";
  return formatBytes(bytes);
}

/**
 * One bar fill: the true proportion, with no floor and no suppression.
 *
 * Both of the obvious adjustments were tried and are wrong:
 *
 * - A **minimum width** (the first version floored every bar at 2px so a tiny
 *   entry still drew something) put 18 of 20 children at exactly the floor
 *   against a 32.9 GB sibling. A column of identical stubs says "these are all
 *   the same" when the truth is "these are all negligible" -- it manufactures
 *   a distinction-free reading out of real differences.
 * - **Suppressing** anything under a threshold fixed that for children and
 *   broke the top-level list, where 31 of 50 projects sit under 1% of the
 *   largest: two thirds of the chart vanished, which reads as not-yet-measured
 *   rather than as small.
 *
 * So: the real fraction, always. A 0.4% bar draws a 0.4% sliver, which is
 * what "negligible next to a 33 GB neighbour" honestly looks like, and the
 * exact number sits beside it for anything the eye cannot resolve. A
 * non-linear scale would make small values legible by misstating them, which
 * is the one thing a size chart must not do.
 */
function Fill({ fraction, tone }: { fraction: number; tone: string }) {
  const pct = Math.min(100, Math.max(0, fraction * 100));
  return (
    <span
      className={cn("block h-full rounded-full transition-[width] duration-[240ms]", tone)}
      style={{ width: `${pct}%` }}
    />
  );
}

export function StorageView({ projects }: { projects: ProjectDto[] }) {
  const active = projects.filter((p) => !p.archived);
  const [ascending, setAscending] = useState(false);
  const queryClient = useQueryClient();

  // One query per project. They queue on the backend's blocking pool, so this
  // is a stream of answers rather than 45 simultaneous disk walks.
  const usages = useQueries({
    queries: active.map((p) => ({
      queryKey: ["usage", p.id],
      queryFn: () => api.projectUsage(p.id),
      // A tree does not change size on its own; only a build changes it, and
      // the explicit Re-measure button covers that.
      staleTime: Infinity,
      gcTime: 30 * 60_000,
    })),
  });

  const done = usages.filter((q) => q.data !== undefined);
  const pending = usages.filter((q) => q.isLoading).length;
  const total = done.reduce((sum, q) => sum + (q.data?.bytes ?? 0), 0);
  const largest = Math.max(1, ...done.map((q) => q.data?.bytes ?? 0));

  const rows = active
    .map((p, i) => ({ project: p, query: usages[i] }))
    .sort((a, b) => {
      const av = a.query?.data?.bytes;
      const bv = b.query?.data?.bytes;
      // Unmeasured rows sink to the bottom in BOTH directions: they are not
      // "zero bytes", they are "not known yet", and treating them as zero
      // would park every pending row at the top of an ascending sort.
      if (av === undefined && bv === undefined) {
        return a.project.name.localeCompare(b.project.name);
      }
      if (av === undefined) return 1;
      if (bv === undefined) return -1;
      return ascending ? av - bv : bv - av;
    });

  const remeasure = () => {
    for (const p of active) {
      void queryClient.invalidateQueries({ queryKey: ["usage", p.id] });
    }
    // Drop every open drill-down too, or an expanded folder keeps showing the
    // sizes it had before the rebuild that prompted the re-measure.
    void queryClient.invalidateQueries({ queryKey: ["children"] });
  };

  return (
    <div className="@container mx-auto flex w-full max-w-[1180px] flex-col gap-5">
      <header className="px-0.5">
        <h2 className="text-[34px] font-bold leading-tight tracking-[0.374px] text-ink-strong">
          Storage
        </h2>
        <p className="mt-1 text-[15px] tracking-[-0.24px] text-ink/75">
          {/* States what was measured AND what was not. "12.4 GB across 40
              projects" while five are still walking is a smaller number than
              the truth, presented as the whole truth. */}
          {done.length === 0 && pending > 0
            ? `Measuring ${pending} project${pending === 1 ? "" : "s"}…`
            : done.length === 0
              ? "Nothing measured yet."
              : `${formatBytes(total)} across ${done.length} of ${active.length} projects` +
                (pending > 0 ? ` · ${pending} still measuring` : "")}
        </p>
      </header>

      <section aria-labelledby="storage-projects">
        <SectionTitle
          id="storage-projects"
          action={
            <div className="flex items-center gap-1">
              {/* A control, not a caption. It used to be a bare ghost label
                  with no icon and no tint, and its visible text stated the
                  CURRENT state ("Largest first") while its aria-label stated
                  the ACTION -- so the screen-reader user got the clearer of
                  the two. Both now say the state, and the arrow says which
                  way, the same as a sorted column header. */}
              <Button
                size="sm"
                variant="outline"
                onClick={() => setAscending((v) => !v)}
                aria-label={`Sorted ${ascending ? "smallest" : "largest"} first. Activate to reverse.`}
              >
                <ArrowDownWideNarrow
                  size={12}
                  aria-hidden
                  className={cn("transition-transform duration-[140ms]", ascending && "-scale-y-100")}
                />
                {ascending ? "Smallest first" : "Largest first"}
              </Button>
              <Button size="sm" onClick={remeasure} className="text-ink/70">
                <RefreshCw size={12} aria-hidden />
                Re-measure
              </Button>
            </div>
          }
        >
          By project
        </SectionTitle>

        <Card>
          {active.length === 0 ? (
            <CardEmpty>
              No projects to measure. Add one and its size shows up here.
            </CardEmpty>
          ) : (
            rows.map(({ project, query }) => (
              <ProjectRow
                key={project.id}
                project={project}
                bytes={query?.data?.bytes}
                skipped={query?.data?.skipped ?? 0}
                failed={query?.isError ?? false}
                largest={largest}
              />
            ))
          )}
        </Card>
      </section>
    </div>
  );
}

function ProjectRow({
  project,
  bytes,
  skipped,
  failed,
  largest,
}: {
  project: ProjectDto;
  bytes: number | undefined;
  skipped: number;
  failed: boolean;
  largest: number;
}) {
  const [open, setOpen] = useState(false);
  const select = useUi((s) => s.select);
  const tone = band(bytes ?? 0);
  const expanded = open;

  return (
    <div className="border-b border-rule/60 last:border-b-0">
      <button
        type="button"
        onClick={() => {
          setOpen((v) => !v);
          select(project.id);
        }}
        aria-expanded={open}
        className={cn(
          "flex min-h-[56px] w-full items-center gap-3 px-4 py-2.5 text-left",
          "transition-colors duration-[120ms] ease-[var(--ease-standard)]",
          "cursor-pointer hover:bg-raised",
          "focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent",
        )}
      >
        <ChevronRight
          size={14}
          aria-hidden
          className={cn(
            "shrink-0 text-ink/45 transition-transform duration-[140ms] ease-[var(--ease-standard)]",
            open && "rotate-90",
          )}
        />
        <ProjectIcon project={project} size={22} className="shrink-0" />

        <div className="min-w-0 flex-1">
          <div className="truncate text-[17px] tracking-[-0.408px] text-ink-strong">
            {project.name}
          </div>
          {/* The bar is `aria-hidden`: the value beside it is the accessible
              fact. A progress role here would announce a percentage of an
              arbitrary maximum, which tells a listener nothing.

              Hidden while this row is expanded, because the children below it
              draw the same span at the same width. Showing both put a 33.1 GB
              parent bar directly above a 32.9 GB child bar one ninth its
              length -- the screen's headline insight, that a build directory
              IS the project's weight, contradicted by its own chart. */}
          {!expanded && (
            <div
              aria-hidden
              data-size-bar
              className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-fill-4"
            >
              {bytes !== undefined && (
                <Fill fraction={bytes / largest} tone={tone.bar} />
              )}
            </div>
          )}
        </div>

        <span
          className={cn("tnum shrink-0 whitespace-nowrap text-[15px]", tone.text)}
          // The band name is spoken here, so a listener hears "very large"
          // rather than a number they have no scale for.
          aria-label={
            bytes === undefined
              ? undefined
              : `${size(bytes)}, ${tone.label}` +
                (skipped > 0 ? `, ${skipped} entries not counted` : "")
          }
        >
          {failed ? (
            <span className="text-signal">could not read</span>
          ) : bytes === undefined ? (
            <span className="text-ink/70">measuring…</span>
          ) : (
            size(bytes)
          )}
        </span>
      </button>

      {open && <Breakdown path={project.root} depth={0} />}
    </div>
  );
}

/**
 * One level of the drill-down, recursive.
 *
 * Each level fetches its children only once opened, so expanding a project
 * never walks a tree the user did not ask about.
 */
function Breakdown({ path, depth }: { path: string; depth: number }) {
  const { data, isLoading, isError, error } = useQuery({
    queryKey: ["children", path],
    queryFn: () => api.folderChildren(path),
    staleTime: Infinity,
    gcTime: 30 * 60_000,
  });
  const indent = { paddingLeft: `${44 + depth * 18}px` };

  if (isLoading) {
    return (
      <p role="status" className="py-2 pr-4 text-[13px] text-ink/70" style={indent}>
        Measuring…
      </p>
    );
  }
  if (isError) {
    return (
      <p role="alert" className="py-2 pr-4 text-[13px] text-signal" style={indent}>
        {(error as { message?: string })?.message ?? "Could not read this folder."}
      </p>
    );
  }

  const children = data ?? [];
  if (children.length === 0) {
    return (
      <p className="py-2 pr-4 text-[13px] text-ink/70" style={indent}>
        Nothing inside.
      </p>
    );
  }

  // Cap the list: a node_modules with 900 packages is a wall, and the answer
  // to "what is big" is always in the first few. The remainder is stated
  // rather than silently dropped.
  const SHOWN = 30;
  const largest = Math.max(1, ...children.map((c) => c.usage.bytes));
  const rest = children.length - SHOWN;

  return (
    <ul className="bg-field/40">
      {children.slice(0, SHOWN).map((child) => (
        <ChildRow key={child.path} child={child} depth={depth} largest={largest} />
      ))}
      {rest > 0 && (
        <li className="py-1.5 pr-4 text-[13px] text-ink/70" style={indent}>
          {rest} smaller {rest === 1 ? "entry" : "entries"} not shown
        </li>
      )}
    </ul>
  );
}

function ChildRow({
  child,
  depth,
  largest,
}: {
  child: DirChildDto;
  depth: number;
  largest: number;
}) {
  const [open, setOpen] = useState(false);
  const tone = band(child.usage.bytes);
  const indent = { paddingLeft: `${44 + depth * 18}px` };

  const body = (
    <>
      {child.isDir ? (
        <ChevronRight
          size={12}
          aria-hidden
          className={cn(
            "shrink-0 text-ink/40 transition-transform duration-[140ms] ease-[var(--ease-standard)]",
            open && "rotate-90",
          )}
        />
      ) : (
        <span aria-hidden className="w-3 shrink-0" />
      )}
      <span
        className={cn(
          "min-w-0 flex-1 truncate text-[15px] @min-[560px]:flex-none @min-[560px]:basis-[38%]",
          child.isDir ? "text-ink/85" : "text-ink/70",
        )}
      >
        {child.name}
      </span>
      {/* Same full-width track as the parent row, so one expanded group is
          one chart at one scale. A fixed 96px track made every child look
          minor next to its own parent. */}
      <span
        aria-hidden
        data-size-bar
        className="hidden h-1 min-w-0 flex-1 overflow-hidden rounded-full bg-fill-4 @min-[560px]:block"
      >
        <Fill fraction={child.usage.bytes / largest} tone={tone.bar} />
      </span>
      <span className={cn("tnum w-20 shrink-0 text-right text-[13px]", tone.text)}>
        {size(child.usage.bytes)}
      </span>
    </>
  );

  return (
    <li>
      {child.isDir ? (
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          aria-label={`${child.name}, ${size(child.usage.bytes)}, ${tone.label}`}
          style={indent}
          className={cn(
            "flex min-h-[32px] w-full items-center gap-2 py-1 pr-4 text-left",
            "cursor-pointer transition-colors duration-[120ms] ease-[var(--ease-standard)]",
            "hover:bg-raised",
            "focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent",
          )}
        >
          {body}
        </button>
      ) : (
        // A file cannot be expanded, so it is not a button. A control that
        // looks identical to a live one but does nothing is the defect this
        // avoids -- the same rule the dashboard's Row follows.
        <div
          style={indent}
          className="flex min-h-[32px] w-full items-center gap-2 py-1 pr-4"
        >
          {body}
        </div>
      )}
      {open && child.isDir && <Breakdown path={child.path} depth={depth + 1} />}
    </li>
  );
}
