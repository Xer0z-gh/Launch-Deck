/**
 * The dashboard: every important number about every project, on one screen,
 * answered from real measurements.
 *
 * # What earns a place here
 *
 * The app has three jobs -- RUN, DEBUG, BUILD PROMPTS -- and each section
 * below answers a question one of them asks on a normal morning:
 *
 * - **Now**: how many are live, how many watched surfaces are down, what died
 *   in the last day, how big the library is. (RUN, DEBUG)
 * - **Live now**: the actual processes, with their actual CPU, memory and
 *   uptime, and a Stop for each. (RUN)
 * - **Watched**: every configured status source and its real reading. (RUN)
 * - **Recent activity**: what ran, how it ended, how long it took. (DEBUG)
 * - **Library**: what this machine is made of, by language. (RUN -- finding
 *   the thing you want is the first half of running it.)
 *
 * # No fabricated numbers
 *
 * Every value is measured or absent. A project with no metrics sample shows
 * an em dash, never a zero; a surface with no reading yet says so; a card
 * with nothing to report says that in words. The counts are derived from the
 * same live queries the rest of the app uses -- the dashboard shares the
 * chips' `["surface", id]` cache rather than probing a second time, so
 * watching everything still costs one probe per project per minute.
 */
import { useQueries, useQuery } from "@tanstack/react-query";
import { Activity, ArrowRight, ExternalLink, ScrollText, Square } from "lucide-react";

import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { formatAge, formatDuration, formatRelative, formatUptime } from "@/lib/format";
import { api, isLive, type ProjectDto, type RunRecordDto } from "@/lib/ipc";
import { formatBytes, formatCpu } from "@/lib/units";
import { useRuntime } from "@/state/runtime";
import { useUi } from "@/state/ui";

import { ProjectIcon } from "@/features/projects/ProjectIcon";
import { useStopProject } from "@/features/projects/useProjects";

import { Card, CardEmpty, Row, SectionTitle, StatTile } from "./primitives";

/** Sentence-cases a clause so one phrase serves the header and a tile. */
function capitalise(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** Outcomes rendered with the signal colour: something actually went wrong. */
const BAD_OUTCOMES = new Set(["failed", "failed_to_start"]);

export function DashboardView({ projects }: { projects: ProjectDto[] }) {
  const states = useRuntime((s) => s.states);
  const metrics = useRuntime((s) => s.metrics);
  const select = useUi((s) => s.select);
  const openLogs = useUi((s) => s.openLogs);
  const setCollection = useUi((s) => s.setCollection);
  const setDestination = useUi((s) => s.setDestination);
  const stop = useStopProject();

  const failures = useQuery({
    queryKey: ["overnight"],
    queryFn: () => api.overnightReport(),
    refetchInterval: 5 * 60_000,
    refetchOnWindowFocus: true,
  });
  const activity = useQuery({
    queryKey: ["recent-runs"],
    queryFn: () => api.recentRuns(),
    refetchInterval: 60_000,
    refetchOnWindowFocus: true,
  });

  const wired = projects.filter((p) => p.surface !== null);
  const readings = useQueries({
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
  const died = (failures.data ?? []).filter((r) => byId.has(r.projectId));
  // A run row keeps `running` until something writes its finish. When the
  // supervisor says the project is not live, that write never happened --
  // the app was killed or the machine slept. Not a failure, but not
  // "nothing happened" either, so the header states it rather than
  // rounding it away.
  const unfinished = (activity.data ?? []).filter(
    (r) => r.outcome === "running" && byId.has(r.projectId) && !isLive(states[r.projectId]),
  );

  // ONE classification of every watched surface, computed once and read by
  // the header sentence, the tile and its detail line alike.
  //
  // The audit that forced this found three counts of the same seven rows in
  // one frame: the header said "all 5 watched apps are answering" (probes
  // still resolving), the tile detail said "All 7 watched apps answering"
  // (asserted from configuration, never measured), and the card showed
  // Local-AI down. Four verdicts, and a row can only be one of them:
  //
  //   pending - no reading yet. Not a verdict; it is the absence of one.
  //   down    - a definite negative: transport failure, or a closed port.
  //   up      - it answered: a status code in 2xx-3xx, or an open port.
  //   quiet   - configured with a log file only. It reports an age, never
  //             an answer, so counting it among "answering" would overstate
  //             what was measured. `describeReading` already tones these
  //             dim rather than up; this agrees with it by construction.
  const verdicts = wired.map((p, i) => {
    const r = readings[i]?.data;
    if (!r) return "pending" as const;
    if (p.surface?.url) {
      if (r.httpError !== null) return "down" as const;
      if (r.httpStatus !== null) {
        return r.httpStatus >= 200 && r.httpStatus < 400 ? ("up" as const) : ("down" as const);
      }
    }
    if (p.surface?.probePort != null) {
      return r.portOpen ? ("up" as const) : ("down" as const);
    }
    return "quiet" as const;
  });
  const down = wired.filter((_, i) => verdicts[i] === "down");
  const pending = verdicts.filter((v) => v === "pending").length;
  const up = verdicts.filter((v) => v === "up").length;
  const quiet = verdicts.filter((v) => v === "quiet").length;

  /**
   * The one sentence about watched surfaces, in the one place that decides
   * what it says. A verdict is never asserted while probes are unresolved.
   */
  const surfacesPhrase = (): string => {
    if (wired.length === 0) return "no status sources configured yet";
    if (pending > 0) return `checking ${pending} of ${wired.length} watched apps`;
    if (down.length > 0) {
      return `${down.length} of ${wired.length} watched ${
        wired.length === 1 ? "app" : "apps"
      } ${down.length === 1 ? "is" : "are"} down`;
    }
    const answering = `${up} answering`;
    return quiet > 0
      ? `${answering}, ${quiet} watched by log only`
      : `all ${answering}`;
  };

  // Archived projects are excluded from the count: the library you work in
  // is not the library you have retired.
  const active = projects.filter((p) => !p.archived);

  const openProject = (id: string) => {
    setDestination("projects");
    select(id);
  };

  return (
    <div className="mx-auto flex w-full max-w-[1100px] flex-col gap-7 pb-8">
      <header className="pt-1">
        {/* h2, not h1: the toolbar's product name is the page's h1, and two
            h1s leave a screen-reader user with no top of the document. */}
        <h2 className="text-[34px] font-bold leading-tight tracking-[0.374px] text-ink-strong">
          Dashboard
        </h2>
        {/* One honest sentence, assembled from the same numbers as the tiles. */}
        {/* Assembled from the SAME values as the tiles below, so the first
            line of the screen can never contradict them. Each clause states
            what it counted. */}
        <p className="mt-1 text-[15px] text-ink/60">
          {[
            running.length > 0 ? `${running.length} running` : "nothing running",
            surfacesPhrase(),
            failures.isLoading
              ? "reading run history"
              : died.length > 0
                ? `${died.length} died in the last 24h`
                : "nothing died in the last 24h",
            ...(unfinished.length > 0
              ? [`${unfinished.length} ${unfinished.length === 1 ? "run" : "runs"} ended without recording`]
              : []),
          ].join(" · ")}
        </p>
      </header>

      {/* ---- Now ---------------------------------------------------------- */}
      <section aria-labelledby="dash-now">
        <SectionTitle id="dash-now">Now</SectionTitle>
        <div className="grid grid-cols-2 gap-3 @min-[640px]:grid-cols-4">
          <StatTile
            label="Running"
            value={running.length}
            unit={running.length === 1 ? "app" : "apps"}
            tone={running.length > 0 ? "live" : "neutral"}
            detail={
              running.length > 0
                ? running.map((p) => p.name).join(", ")
                : "Nothing is live on this machine"
            }
            onClick={() => {
              setDestination("projects");
              setCollection({ kind: "running" });
            }}
            action="Show running projects"
          />
          <StatTile
            label={down.length === 1 ? "Surface down" : "Surfaces down"}
            value={down.length}
            unit={down.length === 1 ? "app" : "apps"}
            tone={down.length > 0 ? "alert" : "neutral"}
            // Same sentence as the header, from the same counts -- the two
            // cannot drift, because there is only one of them.
            detail={
              down.length > 0 ? down.map((p) => p.name).join(", ") : capitalise(surfacesPhrase())
            }
            onClick={() => {
              const first = down[0];
              if (first) openProject(first.id);
              else setDestination("projects");
            }}
            action={
              down.length > 0
                ? `Open ${down[0]?.name}`
                : "Go to projects"
            }
          />
          <StatTile
            label="Died · 24h"
            // An em dash while the query is in flight: rendering 0 before the
            // answer arrives states a fact nobody measured.
            value={failures.isLoading ? "—" : died.length}
            unit={failures.isLoading ? undefined : died.length === 1 ? "run" : "runs"}
            tone={died.length > 0 ? "alert" : "neutral"}
            detail={
              failures.isLoading
                ? "Reading run history…"
                : died.length > 0
                  ? `Newest: ${byId.get(died[0]?.projectId ?? "")?.name ?? "unknown"}`
                  : "No failures recorded in the last day"
            }
            // No deaths means no action, and `StatTile` then renders plain
            // text rather than a button that swallows an Enter press.
            onClick={
              died.length > 0
                ? () => {
                    const first = died[0];
                    if (!first) return;
                    setDestination("projects");
                    select(first.projectId);
                    openLogs(first.projectId);
                  }
                : undefined
            }
            action={died.length > 0 ? "Open the newest failure's log" : undefined}
          />
          <StatTile
            label="Library"
            value={active.length}
            unit="projects"
            detail={`${projects.length - active.length} archived · ${wired.length} watched`}
            onClick={() => {
              setDestination("projects");
              setCollection({ kind: "all" });
            }}
            action="Show all projects"
          />
        </div>
      </section>

      {/* ---- Live now ------------------------------------------------------ */}
      <section aria-labelledby="dash-live">
        <SectionTitle id="dash-live">Live now</SectionTitle>
        <Card>
          {running.length === 0 ? (
            <CardEmpty>
              Nothing is running. Start something from the projects list — every
              row has a Run button.
            </CardEmpty>
          ) : (
            running.map((p) => {
              const samples = metrics[p.id];
              const latest = samples?.[samples.length - 1];
              const state = states[p.id];
              const startedAt =
                state?.tag === "running" ? state.startedAt : null;
              return (
                <Row key={p.id}>
                  <ProjectIcon project={p} size={22} />
                  <button
                    type="button"
                    onClick={() => openProject(p.id)}
                    className={cn(
                      "flex min-h-6 min-w-0 flex-1 items-center truncate text-left",
                      "text-[17px] text-ink-strong",
                      "cursor-pointer rounded-control hover:text-accent focus-visible:outline-2",
                      "focus-visible:outline-offset-2 focus-visible:outline-accent",
                    )}
                  >
                    {p.name}
                  </button>
                  {/* Measured or an em dash. A running process that has not
                      produced a sample yet has no CPU figure, and inventing a
                      0.0% would be a lie about a live process. */}
                  <span className="tnum hidden w-20 shrink-0 text-right text-[15px] text-ink/60 @min-[620px]:block">
                    {latest ? formatCpu(latest.cpuPercent) : "—"}
                  </span>
                  <span className="tnum hidden w-24 shrink-0 text-right text-[15px] text-ink/60 @min-[720px]:block">
                    {latest ? formatBytes(latest.memoryBytes) : "—"}
                  </span>
                  <span className="tnum w-16 shrink-0 text-right text-[15px] text-ink/60">
                    {startedAt
                      ? formatUptime((Date.now() - Date.parse(startedAt)) / 1000)
                      : "—"}
                  </span>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => stop.mutate({ id: p.id })}
                    aria-label={`Stop ${p.name}`}
                  >
                    <Square size={11} className="text-signal" />
                    Stop
                  </Button>
                </Row>
              );
            })
          )}
        </Card>
      </section>

      {/* ---- Watched ------------------------------------------------------- */}
      <section aria-labelledby="dash-watched">
        <SectionTitle id="dash-watched">Watched</SectionTitle>
        <Card>
          {wired.length === 0 ? (
            <CardEmpty>
              No status sources configured. Add one from a project’s row menu →
              “Status source…” to watch a URL, a local port or a log file.
            </CardEmpty>
          ) : (
            wired.map((p, i) => {
              const r = readings[i];
              const reading = describeReading(p, r?.data, r?.isFetching ?? false);
              return (
                <Row key={p.id}>
                  <ProjectIcon project={p} size={22} />
                  <button
                    type="button"
                    onClick={() => openProject(p.id)}
                    className={cn(
                      "flex min-h-6 min-w-0 flex-1 items-center truncate text-left",
                      "text-[17px] text-ink-strong",
                      "cursor-pointer rounded-control hover:text-accent focus-visible:outline-2",
                      "focus-visible:outline-offset-2 focus-visible:outline-accent",
                    )}
                  >
                    {p.name}
                  </button>
                  <span
                    className={cn(
                      "tnum shrink-0 text-[15px]",
                      reading.tone === "up" && "text-ink/80",
                      reading.tone === "down" && "text-signal",
                      reading.tone === "dim" && "text-ink/60",
                    )}
                  >
                    {reading.label}
                  </span>
                  {/* The accessory hangs in a fixed slot outside the value
                      column, so every reading in the card shares one right
                      edge whether or not its row has a button. */}
                  <span className="flex w-[72px] shrink-0 justify-end">
                    {p.surface?.url && (
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => void api.openSurfaceWindow(p.id)}
                        aria-label={`Open ${p.name} in an app window`}
                      >
                        <ExternalLink size={11} />
                        Open
                      </Button>
                    )}
                  </span>
                </Row>
              );
            })
          )}
        </Card>
      </section>

      {/* ---- Recent activity ----------------------------------------------- */}
      <section aria-labelledby="dash-activity">
        <SectionTitle
          id="dash-activity"
          action={
            <Button
              size="sm"
              onClick={() => setDestination("projects")}
              className="text-ink/60"
            >
              All projects
              <ArrowRight size={12} />
            </Button>
          }
        >
          Recent activity
        </SectionTitle>
        <Card>
          {activity.isLoading ? (
            <CardEmpty>Reading run history…</CardEmpty>
          ) : (activity.data ?? []).length === 0 ? (
            <CardEmpty>
              No runs recorded yet. History fills in as you launch things from
              here.
            </CardEmpty>
          ) : (
            collapseRuns(
              (activity.data ?? []).filter((r) => byId.has(r.projectId)),
              (id) => isLive(states[id]),
            )
              .slice(0, 8)
              .map(({ record: r, repeats, oldest }) => (
                <ActivityRow
                  key={r.runId}
                  record={r}
                  repeats={repeats}
                  oldest={oldest}
                  project={byId.get(r.projectId)}
                  live={isLive(states[r.projectId])}
                  onOpen={() => {
                    setDestination("projects");
                    select(r.projectId);
                    openLogs(r.projectId);
                  }}
                />
              ))
          )}
        </Card>
      </section>

    </div>
  );
}

/**
 * Collapses runs of consecutive identical rows.
 *
 * Eight rows reading "Auto-Control · no end recorded" say nothing eight
 * times. One row saying it happened eight times says strictly more, in one
 * eighth of the space -- and the count is itself the signal that something
 * is looping.
 */
function collapseRuns(
  records: RunRecordDto[],
  isProjectLive: (id: string) => boolean,
): { record: RunRecordDto; repeats: number; oldest: string }[] {
  // `key` is bookkeeping for the fold, not part of what the caller reads.
  const out: {
    record: RunRecordDto;
    repeats: number;
    oldest: string;
    key: string;
  }[] = [];
  // The genuinely live run is the NEWEST `running` row of a live project;
  // older ones are orphans that outlived their process. Without this they
  // key identically and the card renders "running x18" under a header
  // saying one thing is running.
  const seenLive = new Set<string>();
  const kind = (r: RunRecordDto): string => {
    if (r.outcome !== "running") return r.outcome;
    if (isProjectLive(r.projectId) && !seenLive.has(r.projectId)) {
      seenLive.add(r.projectId);
      return "running";
    }
    return "orphaned";
  };
  // The exit code is part of the identity: two adjacent failures exiting 1
  // and 137 are two different failures, and collapsing them would delete the
  // 137 -- the one worth reading.
  const key = (r: RunRecordDto, k: string) => `${r.projectId}|${k}|${r.exitCode ?? "-"}`;

  for (const r of records) {
    const k = key(r, kind(r));
    const last = out[out.length - 1];
    if (last && last.key === k) {
      last.repeats += 1;
      last.oldest = r.finishedAt ?? r.startedAt;
    } else {
      out.push({ record: r, repeats: 1, oldest: r.finishedAt ?? r.startedAt, key: k });
    }
  }
  return out;
}

function ActivityRow({
  record,
  repeats,
  oldest,
  project,
  live: projectIsLive,
  onOpen,
}: {
  record: RunRecordDto;
  /** How many consecutive identical runs this row stands for. */
  repeats: number;
  /** Timestamp of the OLDEST run in the group, so the span is stated. */
  oldest: string;
  project: ProjectDto | undefined;
  /** Whether the project is live RIGHT NOW, per the supervisor. */
  live: boolean;
  onOpen: () => void;
}) {
  if (!project) return null;
  const bad = BAD_OUTCOMES.has(record.outcome);
  // A history row keeps `running` until something writes its finish. If the
  // supervisor says the project is not live, that write never happened --
  // the app was killed, or the machine slept. Reporting it as "running"
  // would contradict the header two inches above; the honest statement is
  // that its end was never recorded.
  const live = record.outcome === "running" && projectIsLive;
  const unfinished = record.outcome === "running" && !projectIsLive;
  // The name carries the row's facts. `aria-label` on a button replaces its
  // contents, and an audit measured eight rows announcing the identical
  // "Open Auto-Control logs" -- outcome, duration and recency all dropped.
  const outcomeWord = live
    ? "running"
    : unfinished
      ? "no end recorded"
      : record.outcome.replace(/_/g, " ");
  const exitPart =
    record.exitCode !== null && record.exitCode !== 0 ? `, exit ${record.exitCode}` : "";
  const when = formatRelative(record.finishedAt ?? record.startedAt);
  const times = repeats > 1 ? ` ×${repeats}` : "";
  // A group of eighteen spanning sixteen days printed only its newest
  // timestamp, which read as eighteen runs three minutes ago. State the span.
  const newestMs = Date.parse(record.finishedAt ?? record.startedAt);
  const oldestMs = Date.parse(oldest);
  const spans = repeats > 1 && newestMs - oldestMs > 90_000;
  const span = spans ? ` over ${formatAge(oldest)}` : "";

  return (
    <Row
      onClick={onOpen}
      ariaLabel={`${project.name}: ${outcomeWord}${exitPart}${
        repeats > 1 ? `, ${repeats} times${spans ? ` spanning ${formatAge(oldest)}` : ""}` : ""
      }, ${formatDuration(record.durationMs)}, ${when}. Open logs.`}
    >
      <ProjectIcon project={project} size={22} />
      <span className="min-w-0 flex-1 truncate text-[17px] text-ink-strong">
        {project.name}
      </span>
      <span
        className={cn(
          "shrink-0 text-[15px]",
          bad && "text-signal",
          live && "text-accent",
          !bad && !live && "text-ink/60",
        )}
      >
        {live ? (
          <span className="flex items-center gap-1">
            <Activity size={11} aria-hidden />
            running
          </span>
        ) : unfinished ? (
          "no end recorded"
        ) : (
          record.outcome.replace(/_/g, " ")
        )}
        {times}
        {span}
        {record.exitCode !== null && record.exitCode !== 0
          ? ` · exit ${record.exitCode}`
          : ""}
      </span>
      <span className="tnum hidden w-16 shrink-0 text-right text-[15px] text-ink/60 @min-[620px]:block">
        {formatDuration(record.durationMs)}
      </span>
      <span className="tnum w-20 shrink-0 text-right text-[15px] text-ink/60">
        {formatRelative(record.finishedAt ?? record.startedAt)}
      </span>
      <ScrollText size={13} className="shrink-0 text-ink/40" aria-hidden />
    </Row>
  );
}

/** The same honesty rules as the row chip, in the dashboard's wording. */
function describeReading(
  project: ProjectDto,
  data: { httpStatus: number | null; httpMs: number | null; httpError: string | null; portOpen: boolean | null; logMtime: string | null } | undefined,
  isFetching: boolean,
): { label: string; tone: "up" | "down" | "dim" } {
  if (!data) {
    return { label: isFetching ? "checking…" : "not checked", tone: "dim" };
  }
  const cfg = project.surface;
  if (cfg?.url) {
    if (data.httpStatus !== null) {
      const ok = data.httpStatus >= 200 && data.httpStatus < 400;
      return {
        label: `${data.httpStatus} · ${data.httpMs} ms`,
        tone: ok ? "up" : "down",
      };
    }
    if (data.httpError !== null) return { label: "down", tone: "down" };
  }
  if (cfg?.probePort != null) {
    return data.portOpen
      ? { label: `:${cfg.probePort} up`, tone: "up" }
      : { label: `:${cfg.probePort} closed`, tone: "down" };
  }
  if (data.logMtime !== null) {
    return { label: `log ${formatAge(data.logMtime)}`, tone: "dim" };
  }
  return { label: "no signal", tone: "dim" };
}
