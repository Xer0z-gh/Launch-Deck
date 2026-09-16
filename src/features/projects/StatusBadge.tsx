import { cn } from "@/lib/cn";
import type { RunStateDto, RunTag } from "@/lib/ipc";
import { Tooltip } from "@/components/ui/Tooltip";

interface Visual {
  label: string;
  dot: string;
  text: string;
  pulse?: boolean;
}

/**
 * One visual per state. Accent = live, signal = wrong, warn = in-between;
 * neutral states carry no colour at all so the accent keeps its meaning.
 */
const VISUALS: Record<RunTag, Visual> = {
  // Idle is the state of almost every row almost all the time. Printed at the
  // same weight as everything else it became twenty-seven repetitions of a word
  // carrying no information, which is most of why the list read as noisy grey.
  // It recedes until it is a texture, and the states that matter step forward.
  idle: { label: "Idle", dot: "bg-ink/20", text: "text-ink/30" },
  starting: { label: "Starting", dot: "bg-accent", text: "text-accent", pulse: true },
  running: { label: "Running", dot: "bg-accent", text: "text-accent" },
  stopping: { label: "Stopping", dot: "bg-warn", text: "text-warn", pulse: true },
  exited: { label: "Exited", dot: "bg-ink/25", text: "text-ink/40" },
  crashed: { label: "Crashed", dot: "bg-signal", text: "text-signal" },
  restarting: { label: "Restarting", dot: "bg-warn", text: "text-warn", pulse: true },
};

export function StatusBadge({
  state,
  labelClassName,
}: {
  state: RunStateDto | undefined;
  /**
   * Applied to the text label only. The table passes a container query here so
   * the badge degrades to just its dot when the column has no room for words.
   * Left undefined everywhere else, so every other call site is unaffected --
   * container queries would never match in a subtree with no `@container`
   * ancestor, which would hide the label permanently.
   */
  labelClassName?: string;
}) {
  const tag: RunTag = state?.tag ?? "idle";
  const visual = VISUALS[tag];

  let label = visual.label;
  if (tag === "crashed" && state?.code !== undefined) label = `Crashed (${state.code})`;
  if (tag === "restarting" && state?.attempt !== undefined)
    label = `Restarting #${state.attempt}`;

  // A process exists 11 ms after the click, but `npm run dev` takes seconds to
  // bind a port. Reporting "Running" for that whole window is right about the
  // process and wrong about the service -- and people act on it, click through
  // to a browser, and find nothing there.
  //
  // `healthy === false` means a readiness signal is expected and has not
  // arrived yet. `null` means there is nothing to wait for (a script, a build),
  // and must read exactly like plain Running rather than as a warning.
  const starting = tag === "running" && state?.healthy === false;
  if (starting) label = "Starting";

  const badge = (
    <span className={cn("inline-flex items-center gap-1.5 text-xs", visual.text)}>
      <span
        className={cn(
          "h-1.5 w-1.5 shrink-0 rounded-full",
          visual.dot,
          (visual.pulse || starting) && "animate-pulse",
        )}
      />
      <span className={cn("truncate", labelClassName)}>{label}</span>
    </span>
  );

  if (starting) {
    return (
      <Tooltip label="Process is up but not serving yet — waiting for it to bind a port">
        {badge}
      </Tooltip>
    );
  }

  // A crash with a diagnosed cause explains itself on hover.
  if (tag === "crashed" && state?.reason) {
    return <Tooltip label={state.reason}>{badge}</Tooltip>;
  }
  return badge;
}
