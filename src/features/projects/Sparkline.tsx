import { useMemo } from "react";

/**
 * A hand-rolled SVG sparkline.
 *
 * Deliberately not a charting library. They rebuild their SVG tree on every
 * data change, and at 1 Hz across every visible row that is measurable jank for
 * what amounts to one `<polyline>`. A library earns its place on the detail
 * pages, where charts are large, few, and interactive -- not here.
 *
 * Scaling is relative to the series' own peak rather than a fixed ceiling: CPU
 * is a percentage of one core and can legitimately exceed 100 on a multi-core
 * build, so a fixed 100 ceiling would clip exactly the interesting case.
 */
export function Sparkline({
  values,
  width = 56,
  height = 16,
  className,
}: {
  values: number[];
  width?: number;
  height?: number;
  className?: string;
}) {
  const path = useMemo(() => {
    if (values.length < 2) return null;

    // Leave a half-stroke margin so the line is never clipped at the extremes.
    const pad = 1;
    const peak = Math.max(...values, 1);
    const stepX = (width - pad * 2) / (values.length - 1);

    return values
      .map((value, i) => {
        const x = pad + i * stepX;
        const y = height - pad - (value / peak) * (height - pad * 2);
        return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  }, [values, width, height]);

  // One sample is not a trend; render nothing rather than a misleading dot.
  if (path === null) {
    return <span style={{ width, height }} className={className} aria-hidden />;
  }

  return (
    <svg
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      className={className}
      aria-hidden
      focusable="false"
    >
      <path
        d={path}
        fill="none"
        stroke="var(--color-accent)"
        strokeWidth={1.25}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
