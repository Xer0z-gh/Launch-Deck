/**
 * One dialog for every reason a project cannot launch.
 *
 * # Why one dialog and not five
 *
 * There are five blockers, and the temptation is a screen each. But the user's
 * question is always the same -- "why won't this run, and what do I press?" --
 * so this states the reason in one sentence and offers the single action that
 * fixes *that* reason. Five dialogs would be five places for the wording to
 * drift and five things to maintain for a screen seen once per broken project.
 *
 * # What each blocker actually does
 *
 * | blocker     | why                                   | the fix offered            |
 * | ----------- | ------------------------------------- | -------------------------- |
 * | `root`      | the folder is gone or moved           | point it at the new folder |
 * | `setup`     | dependencies are not installed        | run the install command    |
 * | `program`   | the program is not on PATH            | edit the run command       |
 * | `entry`     | the command names a file that is gone | edit the run command       |
 * | `ambiguous` | cargo has several binaries            | pick one, saved as a flag  |
 * | `unbuilt`   | the built artefact is not there yet    | run the build step         |
 *
 * The `ambiguous` case is the one worth reading: picking a binary writes
 * `cargo run --bin <name>` as the project's run command, so the repair is
 * permanent rather than a one-off launch. That is the difference between a
 * launcher and a terminal.
 */
import { useQueryClient } from "@tanstack/react-query";
import { open as openFolderPicker } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { cn } from "@/lib/cn";
import { asIpcError, type ProjectDto, type SetupReportDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

import { useStartProject, useUpdateProject } from "./useProjects";

export function RepairDialog({
  project,
  report,
  open,
  onOpenChange,
  onEditCommand,
}: {
  project: ProjectDto;
  report: SetupReportDto;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Opens the launch-options editor, for the blockers that need a new command. */
  onEditCommand: () => void;
}) {
  const update = useUpdateProject();
  const install = useStartProject();
  const toast = useUi((s) => s.toast);
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);

  const blocker = report.blocker;
  const done = () => {
    // The verdict is stale the moment the cause is addressed.
    void queryClient.invalidateQueries({ queryKey: ["setup-reports"] });
    onOpenChange(false);
  };

  const relocate = async () => {
    setBusy(true);
    try {
      const picked = await openFolderPicker({
        directory: true,
        multiple: false,
        title: `Where is ${project.name} now?`,
      });
      if (typeof picked === "string") {
        await update.mutateAsync({ id: project.id, patch: { root: picked } });
        toast("info", `${project.name} now points at ${picked}`);
        done();
      }
    } catch (e: unknown) {
      toast("error", asIpcError(e).message);
    } finally {
      setBusy(false);
    }
  };

  const chooseBinary = (name: string) => {
    update.mutate(
      { id: project.id, patch: { runCommand: `cargo run --bin ${name}` } },
      {
        onSuccess: () => {
          toast("info", `${project.name} will run the ${name} binary`);
          done();
        },
        onError: (e: unknown) => toast("error", asIpcError(e).message),
      },
    );
  };

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title={`${project.name} cannot start`}
      description={report.reason ?? "Something is stopping this project from launching."}
    >
      <div className="flex flex-col gap-4 p-5">
        {/* The evidence, always. A repair dialog that does not show what it
            looked at is asking to be trusted rather than read. */}
        <dl className="flex flex-col gap-2 text-[13px]">
          <Row label="Folder" value={project.root} mono missing={blocker === "root"} />
          {project.runCommand && (
            <Row
              label="Run command"
              value={project.runCommand}
              mono
              missing={
                blocker === "program" || blocker === "entry" || blocker === "unbuilt"
              }
            />
          )}
          {report.lastFailedExit !== null && (
            <Row label="Last run" value={`failed, exit ${report.lastFailedExit}`} />
          )}
        </dl>

        {blocker === "ambiguous" && (
          <div className="flex flex-col gap-2">
            <p className="text-[13px] text-ink/70">
              Pick the one Run should launch. It is saved as this project's run
              command, so this only has to be answered once.
            </p>
            <div className="flex flex-wrap gap-1.5">
              {report.choices.map((name) => (
                <Button
                  key={name}
                  size="sm"
                  variant="outline"
                  disabled={update.isPending}
                  onClick={() => chooseBinary(name)}
                  className="font-mono"
                >
                  {name}
                </Button>
              ))}
            </div>
          </div>
        )}

        {blocker === "setup" && report.fixCommand && (
          <p className="text-[13px] leading-relaxed text-ink/70">
            Launch Deck can run{" "}
            <code className="font-mono text-ink-strong">{report.fixCommand}</code> for
            you. It runs in the project folder and streams into the log panel like
            any other step.
          </p>
        )}

        {blocker === "program" && (
          <p className="text-[13px] leading-relaxed text-ink/70">
            Nothing on PATH answers to that name. Either the tool is not
            installed, or this project should be launched a different way — the
            run command is editable below.
          </p>
        )}

        {blocker === "entry" && (
          <p className="text-[13px] leading-relaxed text-ink/70">
            The runner guessed this entry point from the project's shape and the
            file is not there. Point the run command at the real one.
          </p>
        )}

        {blocker === "unbuilt" && (
          <p className="text-[13px] leading-relaxed text-ink/70">
            This runner launches an already-built binary rather than
            recompiling on every Run — that is what makes starting it instant.
            The binary is not there yet, so it has to be built once.
          </p>
        )}

        {blocker === "root" && (
          <p className="text-[13px] leading-relaxed text-ink/70">
            Nothing is at that path any more. If the project moved, point Launch
            Deck at its new home — the run history, status source and notes all
            survive. If it is genuinely gone, remove it from the row menu.
          </p>
        )}

        <div className="flex flex-wrap justify-end gap-2 pt-1">
          <Button onClick={() => onOpenChange(false)}>Close</Button>

          {blocker === "root" && (
            <Button variant="primary" disabled={busy} onClick={() => void relocate()}>
              {busy ? "Choosing…" : "Locate folder…"}
            </Button>
          )}

          {blocker === "setup" && report.fixCommand && (
            <Button
              variant="primary"
              disabled={install.isPending}
              onClick={() => {
                install.mutate(
                  { id: project.id, lifecycle: "install" },
                  { onSettled: done },
                );
              }}
            >
              Run install
            </Button>
          )}

          {blocker === "unbuilt" && project.supported.includes("build") && (
            <Button
              variant="primary"
              disabled={install.isPending}
              onClick={() => {
                install.mutate(
                  { id: project.id, lifecycle: "build" },
                  { onSettled: done },
                );
              }}
            >
              Build it
            </Button>
          )}

          {(blocker === "program" || blocker === "entry" || blocker === "unbuilt") && (
            <Button
              variant="primary"
              onClick={() => {
                onOpenChange(false);
                onEditCommand();
              }}
            >
              Edit launch options…
            </Button>
          )}
        </div>
      </div>
    </Dialog>
  );
}

/** One labelled fact, with the offending one marked. */
function Row({
  label,
  value,
  mono = false,
  missing = false,
}: {
  label: string;
  value: string;
  mono?: boolean;
  missing?: boolean;
}) {
  return (
    <div className="flex flex-col gap-0.5 border-b border-rule/60 pb-2 last:border-b-0">
      <dt className="text-[13px] text-ink/70">{label}</dt>
      <dd
        className={cn(
          "break-words text-[13px]",
          mono && "font-mono text-[12px] leading-relaxed",
          // Colour is never the only carrier: the dialog's description says
          // what is wrong in words, and this only underlines which line it
          // was talking about.
          missing ? "text-signal" : "text-ink-strong",
        )}
      >
        {value}
      </dd>
    </div>
  );
}
