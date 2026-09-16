/**
 * One place that decides what "Run" means.
 *
 * # Why this is a hook and not a call site
 *
 * There are now four controls that launch a project -- the list row, the grid
 * tile, the dashboard and the manual panel -- and one of them being wrong is
 * invisible until you use that particular one. The rule that has to hold
 * everywhere:
 *
 *   A saved web app does not start a process.
 *
 * Its runner declares `cmd /c start` as a fallback for anything outside the
 * app, but routing the UI through that would spawn a shell that exits
 * immediately, recording a run that "died" a fraction of a second after it
 * began. The activity list would fill with deaths that never happened, the
 * overnight report would count them as failures, and the fix would have to be
 * applied at four call sites. So the branch lives here, once.
 */
import { useMutation } from "@tanstack/react-query";

import { api, asIpcError, isWebApp, type ProjectDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

import { useStartProject } from "./useProjects";

export function useLaunch() {
  const start = useStartProject();
  const toast = useUi((s) => s.toast);

  /** Opens a web app in its embedded window, reporting failures like a launch. */
  const open = useMutation({
    mutationFn: (id: string) => api.openSurfaceWindow(id),
    onError: (e: unknown) => toast("error", `Open: ${asIpcError(e).message}`),
  });

  return {
    /**
     * Launches a project the way that project is launched.
     *
     * `lifecycle` picks a runner step other than `run` (dev, build, test) and
     * is ignored for web apps, which have exactly one thing they can do.
     */
    run(project: ProjectDto, lifecycle?: string) {
      if (isWebApp(project)) {
        open.mutate(project.id);
        return;
      }
      start.mutate(lifecycle ? { id: project.id, lifecycle } : { id: project.id });
    },
    /** True while either path is in flight, for disabling the control. */
    get isPending() {
      return start.isPending || open.isPending;
    },
  };
}

/**
 * The runner steps a project can be launched with, in the order they are
 * offered.
 *
 * `install` is deliberately excluded: it is not a way to run the project, it
 * is the repair action the Run button already turns into when setup is
 * missing, and listing it beside `dev` invites running it by accident.
 */
const LIFECYCLE_ORDER = ["run", "dev", "build", "test"] as const;

/** Human labels for the lifecycles, so the menu does not show bare ids. */
export const LIFECYCLE_LABEL: Record<string, string> = {
  run: "Run",
  dev: "Dev server",
  build: "Build",
  test: "Test",
};

/**
 * Which extra launch steps this project supports, beyond the default Run.
 *
 * Reads the runner's own `supported` list rather than assuming a fixed set --
 * a CMake project has `build` and no `dev`, and offering a step the runner
 * cannot perform produces an error that looks like the app is broken.
 */
export function extraLifecycles(project: ProjectDto): string[] {
  if (isWebApp(project)) return [];
  return LIFECYCLE_ORDER.filter(
    (l) => l !== "run" && project.supported.includes(l),
  );
}
