/**
 * Formatting for resource readouts.
 *
 * These return a plain ASCII space between the number and its unit. Keeping
 * "188" and "MB" on one line is a *layout* concern, so it is solved in CSS with
 * `whitespace-nowrap` on the cell that renders the value -- not by smuggling an
 * invisible U+00A0 into the string. An invisible character in a source file is
 * something no reviewer can see and any reformat can destroy.
 */

/** Shown wherever a value is absent, so empty cells read as deliberate. */
const NONE = "—";

/** Bytes → "812 MB", "1.4 GB". Binary units, because that is what task managers show. */
export function formatBytes(bytes: number): string {
  if (bytes <= 0) return NONE;
  const mb = bytes / 1024 / 1024;
  if (mb < 1) return `${Math.round(bytes / 1024)} KB`;
  if (mb < 1024) return `${Math.round(mb)} MB`;
  return `${(mb / 1024).toFixed(1)} GB`;
}

/**
 * CPU percentage → "4%", "132%".
 *
 * Values above 100 are shown as-is rather than clamped: the figure is a
 * percentage of a single core, so a bundler using four of them reads as ~400%,
 * and that is the most informative thing about it.
 */
export function formatCpu(percent: number): string {
  if (percent < 0.5) return "0%";
  return `${Math.round(percent)}%`;
}

/** Ports → "5173", "3000, 5173", or an em dash. */
export function formatPorts(ports: number[]): string {
  return ports.length === 0 ? NONE : ports.join(", ");
}
