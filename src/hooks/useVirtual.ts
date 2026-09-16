import { useCallback, useState, type UIEvent } from "react";

export interface VirtualWindow {
  /** Index of the first rendered row. */
  start: number;
  /** Index one past the last rendered row. */
  end: number;
  /** Pixel offset to translate the rendered slice by. */
  offsetY: number;
  /** Total scrollable height. */
  totalHeight: number;
  /** Attach to the scroll container's onScroll. */
  onScroll: (e: UIEvent<HTMLElement>) => void;
  /** Inform the hook of the viewport height (from a ref callback). */
  setViewport: (height: number) => void;
}

/**
 * Fixed-row-height windowing, sized for the log viewer: 5,000 rows of 20px is
 * trivial arithmetic but far too many live DOM nodes. ~40 rendered rows replace
 * them. No dependency and no measurement pass — row height is a constant by
 * design, which is also what keeps scrollTop math exact for follow-mode.
 */
export function useVirtual(rowCount: number, rowHeight: number, overscan = 12): VirtualWindow {
  const [scrollTop, setScrollTop] = useState(0);
  const [viewport, setViewportState] = useState(400);

  const onScroll = useCallback((e: UIEvent<HTMLElement>) => {
    setScrollTop(e.currentTarget.scrollTop);
  }, []);

  const setViewport = useCallback((height: number) => {
    setViewportState((prev) => (prev === height ? prev : height));
  }, []);

  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const visible = Math.ceil(viewport / rowHeight) + overscan * 2;
  const end = Math.min(rowCount, start + visible);

  return {
    start,
    end,
    offsetY: start * rowHeight,
    totalHeight: rowCount * rowHeight,
    onScroll,
    setViewport,
  };
}
