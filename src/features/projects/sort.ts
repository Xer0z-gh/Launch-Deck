/**
 * Column sorting, Task-Manager style: click a header to sort, click again to
 * reverse.
 *
 * The comparators live here rather than in the table so they can be reasoned
 * about (and tested) without a DOM, and so the table stays presentational.
 *
 * # Two decisions worth knowing about
 *
 * **An explicit sort is obeyed literally.** The default ordering floats pinned
 * and favourite projects to the top, which is what you want when you have not
 * asked for anything in particular. But once you click "Name", a row appearing
 * out of alphabetical order because it happens to be pinned is a bug as far as
 * your eyes are concerned. So pinning applies to the default order only.
 *
 * **Missing values always sort last, in both directions.** A project that has
 * never run has no launch date; ascending should not open with a wall of
 * "never" before the first real value, and neither should descending. "Absent"
 * is not a small value, it is the absence of one.
 */
import type { MetricsDto, ProjectDto, RunStateDto, RunTag } from "@/lib/ipc";

export type SortKey =
  | "status"
  | "name"
  | "kind"
  | "cpu"
  | "uptime"
  | "launched"
  | "used"
  | "added";

export type SortDir = "asc" | "desc";

export interface SortSpec {
  key: SortKey;
  dir: SortDir;
}

/**
 * Status order runs from "most demanding of attention" to least, so ascending
 * puts what is happening now at the top. Alphabetical would open with
 * "Crashed" and bury "Running" in the middle, which is nobody's intent.
 */
const STATUS_RANK: Record<RunTag, number> = {
  running: 0,
  starting: 1,
  restarting: 2,
  stopping: 3,
  crashed: 4,
  exited: 5,
  idle: 6,
};

/** Sentinel for "this project has no value for that column". */
const ABSENT = Number.NEGATIVE_INFINITY;

function timeValue(iso: string | null | undefined): number {
  if (!iso) return ABSENT;
  const t = Date.parse(iso);
  return Number.isNaN(t) ? ABSENT : t;
}

export interface SortContext {
  states: Record<string, RunStateDto | undefined>;
  metrics: Record<string, MetricsDto[] | undefined>;
}

/** The numeric or string value a project contributes to a given column. */
function valueFor(
  project: ProjectDto,
  key: SortKey,
  ctx: SortContext,
): number | string {
  switch (key) {
    case "status":
      return STATUS_RANK[ctx.states[project.id]?.tag ?? "idle"];
    case "name":
      return project.name.toLocaleLowerCase();
    case "kind":
      return project.kindLabel.toLocaleLowerCase();
    case "cpu": {
      const samples = ctx.metrics[project.id];
      const latest = samples?.[samples.length - 1];
      return latest ? latest.cpuPercent : ABSENT;
    }
    case "uptime": {
      // Longer uptime = started earlier. Negated so that "ascending" reads as
      // shortest-first, matching every other column's ascending sense.
      const started = ctx.states[project.id]?.startedAt;
      const t = timeValue(started);
      return t === ABSENT ? ABSENT : -t;
    }
    case "launched":
      return timeValue(project.lastLaunchedAt);
    case "used":
      // Negated so "ascending" reads as least-used-first, matching every
      // other column's ascending sense. A never-run project is 0, which is
      // genuinely the bottom rather than an absent value to sort around.
      return -project.launchCount;
    case "added":
      return timeValue(project.createdAt);
  }
}

/**
 * Builds a comparator for the given sort. Ties fall back to name so the order
 * is total -- otherwise rows with equal values shuffle between renders, which
 * looks like a bug even though the sort is "correct".
 */
export function comparatorFor(
  spec: SortSpec,
  ctx: SortContext,
): (a: ProjectDto, b: ProjectDto) => number {
  const sign = spec.dir === "asc" ? 1 : -1;
  return (a, b) => {
    const va = valueFor(a, spec.key, ctx);
    const vb = valueFor(b, spec.key, ctx);

    // Absent values sink to the bottom regardless of direction.
    const aAbsent = va === ABSENT;
    const bAbsent = vb === ABSENT;
    if (aAbsent !== bAbsent) return aAbsent ? 1 : -1;
    if (aAbsent && bAbsent) return a.name.localeCompare(b.name);

    let cmp: number;
    if (typeof va === "string" && typeof vb === "string") {
      cmp = va.localeCompare(vb);
    } else {
      cmp = (va as number) - (vb as number);
    }
    if (cmp !== 0) return cmp * sign;
    return a.name.localeCompare(b.name);
  };
}

/** True when this sort needs live metric samples, which arrive every second. */
export function sortNeedsMetrics(spec: SortSpec | null): boolean {
  return spec?.key === "cpu";
}

/** Human label for the header's accessible name and tooltip. */
export const SORT_LABEL: Record<SortKey, string> = {
  status: "Status",
  name: "Name",
  kind: "Kind",
  cpu: "CPU",
  uptime: "Uptime",
  launched: "Last run",
  used: "Most used",
  added: "Added",
};
