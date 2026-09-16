/**
 * Warms the PATH-resolution cache for a project's program on selection.
 *
 * Resolving a bare name like `npm` walks every PATH entry against every
 * PATHEXT suffix -- a measured 2.2-4.2 ms on each launch of an
 * interpreter-based project -- and the answer cannot change inside a process.
 * Selection usually precedes Run by enough time to pay that cost early.
 *
 * The byte-priming half this hook once triggered was cut in the 2026-08
 * re-scope: its own controlled measurement put it at 6% of a cold launch,
 * and an in-app A/B across sixteen projects found no difference.
 */
import { useCallback } from "react";

import { api } from "@/lib/ipc";

export function usePrewarm() {
  // The backend resolves each program cheaply and idempotently, so repeated
  // selection costs one IPC message and one cache lookup.
  const now = useCallback((id: string) => {
    void api.prewarmProject(id).catch(() => undefined);
  }, []);

  return { now };
}
