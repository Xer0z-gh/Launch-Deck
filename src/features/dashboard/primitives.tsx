/**
 * The dashboard's building blocks, in the Apple register Tanner asked for on
 * 2026-08-31: large titles, grouped inset cards, generous spacing, softer
 * corners, hairline dividers between rows rather than borders around them.
 *
 * What travels from that reference is STRUCTURE and MATERIAL, not colour --
 * his standing correction (Lathe, 2026-08-25) after a Claude-styled build
 * arrived wearing the wrong palette. Every value here still comes from
 * `tokens.css`, so the dashboard is the same instrument in a calmer suit.
 *
 * The one rule these primitives enforce for the caller: a `Card` states what
 * it measured or says plainly that it measured nothing. There is no "no data"
 * placeholder that could be mistaken for a zero.
 */
import { useId, type ReactNode } from "react";

import { cn } from "@/lib/cn";

/**
 * Section heading above a card group — Apple's grouped-list header.
 *
 * `id` lets the enclosing `<section>` point at this heading with
 * `aria-labelledby` instead of repeating the text in an `aria-label`. Same
 * landmark name, announced once rather than twice.
 */
export function SectionTitle({
  id,
  children,
  action,
}: {
  id?: string;
  children: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="mb-2 flex items-baseline justify-between gap-3 px-0.5">
      {/* h3: one level under the page title, so the outline is
          app (h1) -> page (h2) -> section (h3) with nothing skipped. */}
      <h3 id={id} className="text-[20px] font-semibold tracking-[-0.02em] text-ink-strong">
        {children}
      </h3>
      {action}
    </div>
  );
}

/** A grouped card: one material, hairline top, rows divided from inside. */
export function Card({
  children,
  className,
  padded = false,
}: {
  children: ReactNode;
  className?: string;
  /** Padding for prose/grid content; row lists pad their own rows instead. */
  padded?: boolean;
}) {
  return (
    <div
      className={cn(
        // Flat: the grouped background is what separates a card from the
        // page on iOS. Border and shadow tokens are both no-ops now.
        "overflow-hidden rounded-card bg-panel",
        padded && "p-4",
        className,
      )}
    >
      {children}
    </div>
  );
}

/**
 * One row inside a `Card`. Renders as a button when it has an action, and as
 * a plain row when it does not — a div with a click handler is a defect, and
 * a button that does nothing is worse.
 */
export function Row({
  children,
  onClick,
  ariaLabel,
  className,
}: {
  children: ReactNode;
  onClick?: () => void;
  ariaLabel?: string;
  className?: string;
}) {
  const shared = cn(
    "flex min-h-[44px] w-full items-center gap-3 px-4 py-2.5 text-left",
    "border-b border-rule/60 last:border-b-0",
    className,
  );
  if (!onClick) return <div className={shared}>{children}</div>;
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={ariaLabel}
      className={cn(
        shared,
        "transition-colors duration-[120ms] ease-[var(--ease-standard)]",
        // A pointer and a hover step you can actually see: identical-looking
        // rows where some are buttons and some are not is the defect this
        // fixes. `bg-raised` (not /60) is a real step off `bg-panel`.
        "cursor-pointer hover:bg-raised",
        "focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent",
      )}
    >
      {children}
    </button>
  );
}

/**
 * A headline number with its unit and label.
 *
 * The unit is always spelled out beside the value -- a number that needs a
 * glossary does not belong on the default surface (his 2026-07-30 rule). The
 * label sits ABOVE the number, Apple-style, so a column of tiles scans by
 * name first and value second.
 *
 * # Why there is no `aria-label` here
 *
 * An `aria-label` on a control REPLACES its contents for assistive tech. An
 * audit measured what that cost: "Library / 44 projects" announced as "44
 * projects. Show all projects." -- the visible word "Library" absent from
 * the accessible name, so a voice-control user saying "click Library" got
 * no match, and the detail line silently dropped from every tile. So the
 * name is composed from what is rendered: label, value, unit, then the
 * action phrase in a visually-hidden span. The detail rides `aria-describedby`,
 * which supplements a name instead of replacing it.
 */
export function StatTile({
  label,
  value,
  unit,
  detail,
  tone = "neutral",
  onClick,
  action,
}: {
  label: string;
  value: string | number;
  unit?: string | undefined;
  detail?: string | undefined;
  /** `live` accents a genuinely live count; `alert` marks a real problem. */
  tone?: "neutral" | "live" | "alert";
  // Explicitly `| undefined`: the project builds with
  // `exactOptionalPropertyTypes`, and a caller deciding "no action today"
  // passes undefined rather than omitting the prop.
  onClick?: (() => void) | undefined;
  /** What activating the tile does, e.g. "Show running projects". */
  action?: string | undefined;
}) {
  const detailId = `stat-${useId()}`;
  const body = (
    <>
      <span className="text-[13px] tracking-[-0.078px] text-ink/60">
        {label}
      </span>
      <span className="mt-1 flex items-baseline gap-1">
        <span
          className={cn(
            "tnum text-[28px] font-semibold leading-none tracking-[0.35px]",
            tone === "live" && "text-accent",
            tone === "alert" && "text-signal",
            tone === "neutral" && "text-ink",
          )}
        >
          {value}
        </span>
        {unit && <span className="text-[15px] text-ink/60">{unit}</span>}
      </span>
      {detail && (
        <span
          id={detailId}
          className="mt-1.5 hidden line-clamp-2 text-[13px] leading-snug text-ink/60 @min-[560px]:block"
        >
          {detail}
        </span>
      )}
    </>
  );

  const shared = cn(
    // 88px, and the detail line only appears once the row is wide enough to
    // hold it on one or two lines. At the app's 900x600 minimum the four
    // tiles used to compute 319px tall each, pushing every measured value
    // below the fold on the screen whose job is "state of everything".
    "flex min-h-[88px] flex-col rounded-card bg-panel p-4 text-left",
  );
  // No action means no button: a control that advertises an action it does
  // not perform is worse than plain text.
  if (!onClick) return <div className={shared}>{body}</div>;
  return (
    <button
      type="button"
      onClick={onClick}
      aria-describedby={detail ? detailId : undefined}
      className={cn(
        shared,
        "transition-[background-color,transform] duration-[140ms] ease-[var(--ease-standard)]",
        "cursor-pointer hover:bg-raised active:scale-[0.99]",
        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
      )}
    >
      {body}
      {action && <span className="sr-only">{action}</span>}
    </button>
  );
}

/** What a card says when it genuinely has nothing to report. */
export function CardEmpty({ children }: { children: ReactNode }) {
  return <p className="px-4 py-5 text-[15px] text-ink/60">{children}</p>;
}
