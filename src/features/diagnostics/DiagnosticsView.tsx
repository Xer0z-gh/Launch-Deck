import { useQuery } from "@tanstack/react-query";
import {
  AlertTriangle,
  CheckCircle2,
  Copy,
  Info,
  RefreshCw,
  XCircle,
} from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { api, asIpcError, type CheckSeverity, type DiagnosticsDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

/**
 * The developer diagnostics screen.
 *
 * # Why it leads with verdicts, not data
 *
 * The question this screen answers is "is something wrong", not "what are the
 * numbers". A wall of forty facts makes the reader do the diagnosis, which
 * means the answer is only as good as their memory of what each value is
 * supposed to be. So the backend evaluates every fact it knows how to judge and
 * this screen shows those verdicts first, worst at the top. The raw facts sit
 * underneath for the cases the checks do not cover.
 *
 * # Why it is manual
 *
 * Collecting the report runs `PRAGMA integrity_check` (which walks the entire
 * database) and recursively measures the log directory. Both are cheap once and
 * wasteful on a timer, so this never polls -- it loads when opened and reloads
 * when asked.
 */
const SEVERITY: Record<
  CheckSeverity,
  { icon: typeof CheckCircle2; tone: string; rank: string }
> = {
  fail: { icon: XCircle, tone: "text-signal", rank: "Failing" },
  warn: { icon: AlertTriangle, tone: "text-warn", rank: "Warning" },
  ok: { icon: CheckCircle2, tone: "text-good", rank: "Healthy" },
  info: { icon: Info, tone: "text-ink/45", rank: "Info" },
};

/** Flattens the report into the text a bug report would want pasted into it. */
function asPlainText(report: DiagnosticsDto): string {
  const lines: string[] = [
    "LAUNCH DECK DIAGNOSTICS",
    `Generated: ${report.generatedAt}`,
    "",
    "CHECKS",
  ];
  for (const c of report.checks) {
    lines.push(`  [${c.severity.toUpperCase()}] ${c.label} — ${c.detail}`);
  }
  for (const section of report.sections) {
    lines.push("", section.title.toUpperCase());
    for (const f of section.facts) lines.push(`  ${f.label}: ${f.value}`);
  }
  return lines.join("\n");
}

/** A stable DOM id for a section heading, so its `<section>` can point at it. */
function sectionHeadingId(title: string): string {
  return `diag-${title.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
}

export function DiagnosticsView() {
  const toast = useUi((s) => s.toast);
  const [copied, setCopied] = useState(false);

  const { data, isLoading, isError, error, refetch, isFetching } = useQuery({
    queryKey: ["diagnostics"],
    queryFn: () => api.diagnostics(),
    // Never served from cache: a stale health report is worse than none, because
    // it looks current.
    staleTime: 0,
    gcTime: 0,
    refetchOnMount: "always",
  });

  const copy = async () => {
    if (!data) return;
    try {
      await navigator.clipboard.writeText(asPlainText(data));
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      toast("error", `Could not copy: ${asIpcError(e).message}`);
    }
  };

  const failing = data?.checks.filter((c) => c.severity === "fail") ?? [];
  const warnings = data?.checks.filter((c) => c.severity === "warn") ?? [];

  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-5">
      <header className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-[15px] font-semibold text-ink">Diagnostics</h2>
          <p className="mt-0.5 text-xs text-ink/45">
            {isLoading
              ? "Collecting…"
              : data
                ? `Checked ${data.checks.length} things · ${new Date(
                    data.generatedAt,
                  ).toLocaleTimeString()}`
                : "—"}
          </p>
        </div>
        <Button variant="outline" onClick={() => void copy()} disabled={!data}>
          <Copy size={13} />
          {copied ? "Copied" : "Copy report"}
        </Button>
        <Button variant="outline" onClick={() => void refetch()} disabled={isFetching}>
          <RefreshCw size={13} className={cn(isFetching && "animate-spin")} />
          Refresh
        </Button>
      </header>

      {isError && (
        <p className="rounded-panel border border-signal/40 bg-panel p-4 text-xs text-signal">
          {asIpcError(error).message}
        </p>
      )}

      {isLoading && (
        <div className="flex flex-col gap-2" aria-label="Loading diagnostics">
          {Array.from({ length: 6 }, (_, i) => (
            <div key={i} className="h-12 animate-pulse rounded-panel bg-panel" />
          ))}
        </div>
      )}

      {data && (
        <>
          {/* The headline: one sentence answering the actual question. */}
          <div
            className={cn(
              "rounded-panel border p-4",
              failing.length > 0
                ? "border-signal/40 bg-signal/5"
                : warnings.length > 0
                  ? "border-warn/40 bg-warn/5"
                  : "border-good/30 bg-good/5",
            )}
          >
            <p className="text-[13px] font-medium text-ink">
              {failing.length > 0
                ? `${failing.length} problem${failing.length === 1 ? "" : "s"} found`
                : warnings.length > 0
                  ? `No failures, ${warnings.length} warning${warnings.length === 1 ? "" : "s"}`
                  : "Everything checks out"}
            </p>
            <p className="mt-1 text-xs leading-relaxed text-ink/55">
              {failing.length > 0
                ? "The failing checks below are things that will cause visible misbehaviour."
                : warnings.length > 0
                  ? "Nothing is broken, but these are outside their intended envelope."
                  : "Every check the app knows how to make came back healthy."}
            </p>
          </div>

          {/* Named, so a screen reader can jump straight to the findings
              instead of arrowing through the whole report to find them. */}
          <section className="flex flex-col gap-1.5" aria-label="Findings">
            {data.checks.map((c) => {
              const v = SEVERITY[c.severity];
              const Icon = v.icon;
              return (
                <div
                  key={c.id}
                  className="flex items-start gap-2.5 rounded-panel bg-panel px-3 py-2.5"
                >
                  <Icon size={14} className={cn("mt-0.5 shrink-0", v.tone)} aria-hidden />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-baseline gap-2">
                      <span className="text-[13px] text-ink">{c.label}</span>
                      <span className="shrink-0 text-[10px] uppercase tracking-wide text-ink/30">
                        {c.section}
                      </span>
                    </div>
                    <p className="mt-0.5 break-words text-xs leading-relaxed text-ink/50">
                      {c.detail}
                    </p>
                  </div>
                  <span className={cn("shrink-0 text-[10px] uppercase", v.tone)}>
                    {v.rank}
                  </span>
                </div>
              );
            })}
          </section>

          {/* `aria-labelledby` rather than a repeated `aria-label`: the heading
              is already the region's name, and duplicating it as a string is
              two things to keep in sync. An unnamed <section> is not exposed as
              a landmark at all, so this is the difference between fourteen
              navigable regions and one undifferentiated wall. */}
          {data.sections.map((section) => (
            <section key={section.title} aria-labelledby={sectionHeadingId(section.title)}>
              <h3
                id={sectionHeadingId(section.title)}
                className="mb-1.5 text-[11px] uppercase tracking-wide text-ink/40"
              >
                {section.title}
              </h3>
              <dl className="overflow-hidden rounded-panel bg-panel">
                {section.facts.map((f) => (
                  <div
                    key={f.label}
                    className="flex gap-4 border-b border-rule/60 px-3 py-1.5 last:border-b-0"
                  >
                    <dt className="w-44 shrink-0 text-xs text-ink/45">{f.label}</dt>
                    {/* Paths and versions are the things people copy out of a
                        report, so this text stays selectable even though the
                        rest of the app disables selection. */}
                    <dd className="tnum min-w-0 flex-1 select-text break-all text-xs text-ink/80">
                      {f.value}
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          ))}
        </>
      )}
    </div>
  );
}
