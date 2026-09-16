import { useQueryClient } from "@tanstack/react-query";
import { AppWindow, Archive, Bug, Code2, Download, ExternalLink, FolderOpen, MoreHorizontal, Pin, Play, Radar, RotateCcw, ScrollText, Square, SquarePen, SquareTerminal, Star, Terminal, Trash2, Wrench } from "lucide-react";
import { useState } from "react";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { Menu } from "@/components/ui/Menu";
import { Tooltip } from "@/components/ui/Tooltip";
import {
  api,
  asIpcError,
  canStart,
  canStop,
  isWebApp,
  type ProjectDto,
} from "@/lib/ipc";
import { LaunchOptionsDialog } from "./LaunchOptionsDialog";
import { RepairDialog } from "./RepairDialog";
import { useSetupReport } from "./useSetupReport";
import { SurfaceDialog } from "./SurfaceDialog";
import { extraLifecycles, LIFECYCLE_LABEL, useLaunch } from "./useLaunch";
import { useRuntime } from "@/state/runtime";
import { useUi } from "@/state/ui";

import {
  useRemoveProject,
  useRestartProject,
  useStartProject,
  useStopProject,
  useUpdateProject,
} from "./useProjects";

/**
 * The action cluster every row and card shares: ONE contextual primary action
 * (Run ↔ Stop — the state machine guarantees never both), restart, logs, and
 * an overflow menu for everything else. `compact` renders only the overflow
 * menu, for hosts (the detail bar) that provide the primary actions
 * themselves.
 */
export function RowActions({
  project,
  compact = false,
  minimal = false,
}: {
  project: ProjectDto;
  compact?: boolean;
  /**
   * Grouped-list mode: the primary action and the overflow menu, nothing
   * else. Apple's list rows carry ONE trailing accessory; five naked glyph
   * buttons per row is the single most Windows-looking thing a list can do,
   * and everything hidden here already exists inside the menu.
   */
  minimal?: boolean;
}) {
  const state = useRuntime((s) => s.states[project.id]);
  const openLogs = useUi((s) => s.openLogs);
  const openStudio = useUi((s) => s.openStudio);
  const toast = useUi((s) => s.toast);

  const start = useStartProject();
  // Same mutation, different lifecycle: the install command has been in every
  // runner manifest since the beginning and simply had no way to reach a user.
  const install = useStartProject();
  const stop = useStopProject();
  const restart = useRestartProject();
  const update = useUpdateProject();
  const remove = useRemoveProject();

  const [renameOpen, setRenameOpen] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [surfaceOpen, setSurfaceOpen] = useState(false);
  const [commandOpen, setCommandOpen] = useState(false);
  const [repairOpen, setRepairOpen] = useState(false);
  const [draftName, setDraftName] = useState(project.name);

  const startable = canStart(state);
  const stoppable = canStop(state);

  const queryClient = useQueryClient();
  const setup = useSetupReport(project.id);
  // Only when the project could otherwise be started. A running project's
  // primary action is Stop, and a missing folder is not something an install
  // can fix -- offering one there would just fail differently.
  const needsSetup = startable && (setup?.needsSetup ?? false) && (setup?.rootExists ?? false);
  // `false` only once the report has arrived; an absent report means "not yet
  // known", which must not disable the button on every first render.
  const missingRoot = setup?.rootExists === false;

  /**
   * A knowable reason this will not start.
   *
   * Replaces Run rather than annotating it, for the same reason the setup case
   * already did: offering a launch that is known to fail produces an exit code
   * about a syscall, for a situation the app already understood. Only while the
   * project could otherwise be started -- a running project's primary action is
   * Stop, whatever its folder looks like.
   */
  const desktop = (action: Promise<void>, what: string) => {
    action.catch((e: unknown) => toast("error", `${what}: ${asIpcError(e).message}`));
  };

  // Run goes through the shared hook so a saved web app opens its window
  // instead of spawning a shell that exits immediately. See `useLaunch`.
  const launch = useLaunch();
  const web = isWebApp(project);

  /**
   * A knowable reason this will not start.
   *
   * Replaces Run rather than annotating it, for the same reason the setup case
   * already did: offering a launch that is known to fail produces an exit code
   * about a syscall, for a situation the app already understood. Only while the
   * project could otherwise be started -- a running project's primary action is
   * Stop, whatever its folder looks like -- and never for a web app, which has
   * no folder to miss and no dependencies to install.
   */
  const blocker = startable && !web ? (setup?.blocker ?? null) : null;

  return (
    <div
      className="flex items-center justify-end gap-1"
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => e.stopPropagation()}
    >
      {!compact && (stoppable ? (
        <Tooltip label="Stop">
          <Button
            size="icon"
            aria-label={`Stop ${project.name}`}
            onClick={() => stop.mutate({ id: project.id })}
          >
            {/* Red, not amber: stopping is the destructive action here, and
                warn is reserved for transitional states (stopping/restarting)
                shown in the status column. */}
            <Square size={13} className="text-signal" />
          </Button>
        </Tooltip>
      ) : blocker && blocker !== "setup" ? (
        // Everything except a plain dependency install, which keeps its own
        // one-press button below.
        <Tooltip label={setup?.reason ?? "Cannot start"}>
          <Button
            size="icon"
            aria-label={`${project.name} cannot start: ${setup?.reason ?? "unknown reason"}. Repair.`}
            onClick={() => setRepairOpen(true)}
          >
            <Wrench size={13} className="text-warn" />
          </Button>
        </Tooltip>
      ) : needsSetup ? (
        // Run is REPLACED, not merely annotated. Leaving it as the primary
        // action invites a launch that is already known to fail, and the
        // resulting exit code reads as "the launcher crashed my app" -- which
        // is exactly what this whole feature exists to stop.
        <Tooltip
          label={`${setup?.hint ?? "Setup needed"}${
            setup?.fixCommand ? ` — runs: ${setup.fixCommand}` : ""
          }`}
        >
          <Button
            size="icon"
            aria-label={`Install dependencies for ${project.name}`}
            disabled={install.isPending}
            onClick={() =>
              install.mutate(
                { id: project.id, lifecycle: "install" },
                {
                  // The answer changes the moment the install finishes, and a
                  // stale "needs setup" would keep hiding Run behind a button
                  // the user already pressed.
                  onSettled: () =>
                    void queryClient.invalidateQueries({ queryKey: ["setup-reports"] }),
                },
              )
            }
          >
            <Download size={13} className="text-warn" />
          </Button>
        </Tooltip>
      ) : (
        <Tooltip
          label={
            web
              ? `Open ${project.surface?.url ?? "this web app"}`
              : missingRoot
                ? "Folder no longer exists"
                : project.runCommand ?? "Run"
          }
        >
          <Button
            size="icon"
            aria-label={web ? `Open ${project.name}` : `Run ${project.name}`}
            // A project whose folder was deleted or moved cannot start, and
            // offering Run only produces `The directory name is invalid.
            // (os error 267)` -- an error about a syscall, for a situation the
            // app already knows about. The tooltip says the real thing.
            // A web app has no folder to miss and no setup to need, so neither
            // gate applies to it.
            disabled={web ? !project.surface?.url : !startable || start.isPending || missingRoot}
            onClick={() => launch.run(project)}
          >
            <Play size={13} className="text-accent" />
          </Button>
        </Tooltip>
      ))}

      {!compact && !minimal && (
        <Tooltip label="Restart">
          <Button
            size="icon"
            aria-label={`Restart ${project.name}`}
            // Secondary actions give up their space before the name column
            // becomes unreadable. Both stay reachable in the overflow menu.
            className="hidden @min-[480px]:inline-flex"
            disabled={!stoppable && !startable}
            onClick={() => restart.mutate({ id: project.id })}
          >
            <RotateCcw size={13} />
          </Button>
        </Tooltip>
      )}

      {/* Favourite is a one-click toggle rather than a menu trip, because it is
          the only action here you perform *while browsing* -- deciding a
          project matters is part of scanning the list, not a deliberate
          operation like restarting something. It fills and takes the warn
          colour when set, matching the star already shown beside the name. */}
      {!compact && !minimal && (
        <Tooltip label={project.favorite ? "Remove from favourites" : "Add to favourites"}>
          <Button
            size="icon"
            aria-label={
              project.favorite
                ? `Remove ${project.name} from favourites`
                : `Add ${project.name} to favourites`
            }
            aria-pressed={project.favorite}
            className="hidden @min-[480px]:inline-flex"
            onClick={() =>
              update.mutate({
                id: project.id,
                patch: { favorite: !project.favorite },
              })
            }
          >
            <Star
              size={13}
              className={project.favorite ? "text-warn" : undefined}
              fill={project.favorite ? "currentColor" : "none"}
            />
          </Button>
        </Tooltip>
      )}

      {!compact && !minimal && (
        <Tooltip label="Logs">
          <Button
            size="icon"
            aria-label={`Logs for ${project.name}`}
            className="hidden @min-[560px]:inline-flex"
            onClick={() => openLogs(project.id)}
          >
            <ScrollText size={13} />
          </Button>
        </Tooltip>
      )}

      {project.surface?.url && !compact && !minimal && (
        <Tooltip label={`Open ${project.surface.url}`}>
          <Button
            size="icon"
            aria-label={`Open ${project.name} site`}
            onClick={() => desktop(api.openSurfaceUrl(project.id), "Open site")}
          >
            <ExternalLink size={13} />
          </Button>
        </Tooltip>
      )}

      <Menu
        trigger={
          <Button size="icon" aria-label={`More actions for ${project.name}`}>
            <MoreHorizontal size={13} />
          </Button>
        }
        items={[
          // Restart and Logs lead the menu because the row can hide their
          // buttons when the column is narrow. Duplicating a visible control in
          // the overflow menu is cheap; leaving an action unreachable at some
          // window sizes is not.
          {
            label: "View logs",
            icon: <ScrollText size={13} />,
            onSelect: () => openLogs(project.id),
          },
          {
            label: "Restart",
            icon: <RotateCcw size={13} />,
            disabled: !stoppable && !startable,
            onSelect: () => restart.mutate({ id: project.id }),
          },
          ...extraLifecycles(project).map((lifecycle) => ({
            label: `${LIFECYCLE_LABEL[lifecycle] ?? lifecycle}`,
            icon: <Play size={13} />,
            disabled: !startable || missingRoot,
            onSelect: () => launch.run(project, lifecycle),
          })),
          ...(web
            ? []
            : [
                {
                  label: "Launch options…",
                  icon: <SquareTerminal size={13} />,
                  onSelect: () => setCommandOpen(true),
                },
              ]),
          ...(setup?.blocker
            ? [
                {
                  label: "Why it will not start…",
                  icon: <Wrench size={13} />,
                  onSelect: () => setRepairOpen(true),
                },
              ]
            : []),
          {
            label: "Debug this…",
            icon: <Bug size={13} />,
            onSelect: () => openStudio(project.id, "debug"),
          },
          {
            label: "Build prompt…",
            icon: <SquareTerminal size={13} />,
            onSelect: () => openStudio(project.id),
          },
          {
            label: "Open folder",
            icon: <FolderOpen size={13} />,
            section: true,
            onSelect: () => desktop(api.openFolder(project.id), "Open folder"),
          },
          {
            label: "Open terminal here",
            icon: <Terminal size={13} />,
            onSelect: () => desktop(api.openTerminal(project.id), "Open terminal"),
          },
          {
            label: "Open in VS Code",
            icon: <Code2 size={13} />,
            onSelect: () => desktop(api.openInEditor(project.id), "Open editor"),
          },
          ...(project.surface?.url
            ? [
                {
                  label: "Open in app window",
                  icon: <AppWindow size={13} />,
                  onSelect: () =>
                    desktop(api.openSurfaceWindow(project.id), "Open app window"),
                },
              ]
            : []),
          {
            label: "Status source…",
            icon: <Radar size={13} />,
            onSelect: () => setSurfaceOpen(true),
          },
          {
            label: "Rename…",
            icon: <SquarePen size={13} />,
            section: true,
            onSelect: () => {
              setDraftName(project.name);
              setRenameOpen(true);
            },
          },
          {
            label: project.favorite ? "Unfavourite" : "Favourite",
            icon: <Star size={13} />,
            onSelect: () =>
              update.mutate({ id: project.id, patch: { favorite: !project.favorite } }),
          },
          {
            label: project.pinned ? "Unpin" : "Pin to top",
            icon: <Pin size={13} />,
            onSelect: () =>
              update.mutate({ id: project.id, patch: { pinned: !project.pinned } }),
          },
          {
            label: project.archived ? "Unarchive" : "Archive",
            icon: <Archive size={13} />,
            onSelect: () =>
              update.mutate({ id: project.id, patch: { archived: !project.archived } }),
          },
          {
            label: "Remove…",
            icon: <Trash2 size={13} />,
            danger: true,
            section: true,
            onSelect: () => setConfirmRemove(true),
          },
        ]}
      />

      <SurfaceDialog
        // Keyed so reopening after a save re-reads the project's current
        // config into the fields instead of resurrecting stale local state.
        key={surfaceOpen ? "open" : "closed"}
        project={project}
        open={surfaceOpen}
        onOpenChange={setSurfaceOpen}
      />

      <Dialog
        open={renameOpen}
        onOpenChange={setRenameOpen}
        title="Rename project"
        description="Display name only — the folder on disk is untouched."
      >
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const name = draftName.trim();
            if (name.length > 0 && name !== project.name) {
              update.mutate({ id: project.id, patch: { name } });
            }
            setRenameOpen(false);
          }}
          className="flex flex-col gap-3"
        >
          <Input
            autoFocus
            value={draftName}
            onChange={(e) => setDraftName(e.target.value)}
            aria-label="Project name"
          />
          <div className="flex justify-end gap-2">
            <Button onClick={() => setRenameOpen(false)}>Cancel</Button>
            <Button variant="primary" type="submit">
              Rename
            </Button>
          </div>
        </form>
      </Dialog>

      {commandOpen && (
        <LaunchOptionsDialog
          key={project.id}
          project={project}
          open={commandOpen}
          onOpenChange={setCommandOpen}
        />
      )}

      {setup?.blocker && (
        <RepairDialog
          project={project}
          report={setup}
          open={repairOpen}
          onOpenChange={setRepairOpen}
          onEditCommand={() => setCommandOpen(true)}
        />
      )}

      <Dialog
        open={confirmRemove}
        onOpenChange={setConfirmRemove}
        title={`Remove "${project.name}"?`}
        description="Removes it from Launch Deck and deletes its logs. The project folder on disk is not touched."
      >
        <div className="flex justify-end gap-2">
          <Button onClick={() => setConfirmRemove(false)}>Cancel</Button>
          <Button
            variant="danger"
            className="border border-signal/40"
            onClick={() => {
              remove.mutate({ id: project.id });
              setConfirmRemove(false);
              useRuntime.getState().clearProject(project.id);
            }}
          >
            Remove project
          </Button>
        </div>
      </Dialog>
    </div>
  );
}
