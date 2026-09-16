/** Human formatting for the values the dashboard displays. */

/** Seconds → "3s", "4m 12s", "2h 05m", "3d 4h". */
export function formatUptime(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, "0")}s`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, "0")}m`;
  const d = Math.floor(h / 24);
  return `${d}d ${h % 24}h`;
}

/** ISO timestamp → "just now", "5m ago", "3h ago", "2d ago", else a date. */
export function formatRelative(iso: string | null): string {
  if (!iso) return "never";
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "—";
  const seconds = Math.floor((Date.now() - then) / 1000);
  if (seconds < 45) return "just now";
  if (seconds < 3600) return `${Math.max(1, Math.floor(seconds / 60))}m ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
  if (seconds < 7 * 86400) return `${Math.floor(seconds / 86400)}d ago`;
  return new Date(then).toLocaleDateString();
}

/**
 * ISO timestamp → the shortest honest age: "40s" / "12m" / "3h" / "65d".
 *
 * Distinct from `formatRelative`, which says "2d ago" and then falls back to
 * a calendar date past a week. For a status reading the AGE is the signal --
 * "log 65d" says "nobody has touched this in two months" at a glance, where
 * "log 6/27/2026" makes the reader do the subtraction. Both the row chips and
 * the dashboard call this one, so the same fact cannot render two ways.
 */
export function formatAge(iso: string): string {
  const secs = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (secs < 90) return `${Math.round(secs)}s`;
  if (secs < 90 * 60) return `${Math.round(secs / 60)}m`;
  if (secs < 36 * 3600) return `${Math.round(secs / 3600)}h`;
  return `${Math.round(secs / 86400)}d`;
}

/** ISO timestamp → "14:03:22.481" for log gutters. */
export function formatLogTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  const ss = String(d.getSeconds()).padStart(2, "0");
  const ms = String(d.getMilliseconds()).padStart(3, "0");
  return `${hh}:${mm}:${ss}.${ms}`;
}

/** Milliseconds → "431ms", "3.2s", "1m 04s". */
export function formatDuration(ms: number | null): string {
  if (ms === null || ms < 0) return "—";
  if (ms < 1000) return `${ms}ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)}s`;
  const m = Math.floor(s / 60);
  return `${m}m ${String(Math.floor(s % 60)).padStart(2, "0")}s`;
}
