import { useEffect, useState } from "react";

/**
 * A ticking clock for uptime displays. One interval per subscriber, only while
 * mounted — cards that are not running never pay for it.
 */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}
