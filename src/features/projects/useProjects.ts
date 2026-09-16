/** Server-state hooks: projects list and the mutations that edit it. */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { api, asIpcError, type ProjectDto, type ProjectPatch } from "@/lib/ipc";
import { useRuntime } from "@/state/runtime";
import { useUi } from "@/state/ui";

export function useProjects() {
  return useQuery({
    queryKey: ["projects"],
    queryFn: async () => {
      performance.mark("projects-fetch-start");
      const rows = await api.listProjects();
      performance.mark("projects-arrived");
      return rows;
    },
    staleTime: 5_000,
  });
}

/** Wraps a mutation with invalidation and an error toast. */
function useProjectMutation<TArgs>(fn: (args: TArgs) => Promise<unknown>) {
  const queryClient = useQueryClient();
  const toast = useUi((s) => s.toast);
  return useMutation({
    mutationFn: fn,
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["projects"] }),
    onError: (e: unknown) => toast("error", asIpcError(e).message),
  });
}

/**
 * Starts a project, showing it as starting immediately.
 *
 * # Why the row changes before the backend answers
 *
 * A warm launch measured **73 ms from click to the row showing it running**, of
 * which the backend's own share is **6-16 ms**. The rest is the IPC round trip
 * and the re-render — real work, but none of it is work the user has any reason
 * to wait through. The click has already committed the decision; the only
 * question left is whether it succeeded, and that arrives on its own.
 *
 * So the row flips to `starting` on the click and the real
 * `deck://state` event replaces it a few tens of milliseconds later. Perceived
 * latency goes from 73 ms to one frame.
 *
 * # Why this is not lying to the user
 *
 * `starting` is a real state the run-state machine already has, and it is
 * exactly what the backend is about to report. If the start fails, the failure
 * arrives as a state change plus an error toast and the row corrects itself —
 * the same path a failure took before. The one thing this must never do is
 * claim `running`, which would assert that a process exists when none does.
 *
 * # The rollback is not optional
 *
 * A spawn failure emits `Crashed`, so the row corrects itself. But two failures
 * return *before* anything is spawned and therefore emit nothing at all —
 * `AlreadyRunning`, and a required port already in use. Without a rollback the
 * row would sit at `starting` forever, which is a worse lie than the wait this
 * removes: a stuck spinner claims something is happening. So the previous state
 * is captured and restored on any error.
 */
export function useStartProject() {
  const queryClient = useQueryClient();
  const toast = useUi((s) => s.toast);
  return useMutation({
    mutationFn: (args: { id: string; lifecycle?: string }) =>
      api.startProject(args.id, args.lifecycle),
    onMutate: (args) => {
      const previous = useRuntime.getState().states[args.id];
      useRuntime.getState().applyState(args.id, { tag: "starting" });
      return { previous };
    },
    onError: (e: unknown, args, context) => {
      // Restore exactly what was there, including "nothing": a project that was
      // idle must go back to idle, not to a fabricated state.
      const previous = context?.previous;
      if (previous) useRuntime.getState().applyState(args.id, previous);
      else useRuntime.getState().applyState(args.id, { tag: "idle" });
      toast("error", asIpcError(e).message);
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["projects"] }),
  });
}

export function useStopProject() {
  return useProjectMutation((args: { id: string; force?: boolean }) =>
    api.stopProject(args.id, args.force ?? false),
  );
}

export function useRestartProject() {
  return useProjectMutation((args: { id: string }) => api.restartProject(args.id));
}

export function useUpdateProject() {
  return useProjectMutation((args: { id: string; patch: ProjectPatch }) =>
    api.updateProject(args.id, args.patch),
  );
}

export function useRemoveProject() {
  return useProjectMutation((args: { id: string }) => api.removeProject(args.id));
}


/**
 * The resting order: pinned, then favourites, then whatever you actually use.
 *
 * # Why this changed from alphabetical
 *
 * The old order was pinned → favourites → name. That reads as sensible until
 * you look at the real library: 56 projects, **zero** pinned and **zero**
 * favourited. Both tiers were empty, so the launcher opened on an alphabetical
 * list where the first screen was Auto-Control, Calculator, Chess-Scout — and
 * the thing actually wanted was somewhere in the other 45.
 *
 * Curation was the implicit plan, and it never happened, because tagging 56
 * things by hand is work nobody does. Meanwhile 475 run records sat in the
 * database describing exactly which ones matter. So the order now comes from
 * evidence rather than from intent:
 *
 * 1. **Pinned**, then **favourites** — explicit choices still win outright.
 * 2. **Launched before, most recent first.** A launcher's best guess at what
 *    you want next is what you wanted last.
 * 3. **Never launched, alphabetically.** 24 of the 51 originals were in this
 *    group. They are not hidden -- search still finds them instantly -- they
 *    simply stop occupying the first screen.
 *
 * Recency rather than a blended "frecency" score on purpose: it is one
 * sentence to explain, and a number that needs a glossary does not belong on
 * the default surface. Frequency is available as its own sort ("Most used")
 * for when that is the question being asked.
 */
export function defaultSort(a: ProjectDto, b: ProjectDto): number {
  if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
  if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;

  const aRun = a.lastLaunchedAt;
  const bRun = b.lastLaunchedAt;
  if (aRun && bRun && aRun !== bRun) return aRun < bRun ? 1 : -1;
  // Never-launched sinks below everything that has run, whatever its name.
  if (Boolean(aRun) !== Boolean(bRun)) return aRun ? -1 : 1;
  return a.name.localeCompare(b.name);
}
