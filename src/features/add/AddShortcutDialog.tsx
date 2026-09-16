/**
 * Adds a file, script or folder to the library as something you can launch.
 *
 * # Why this is a thin wrapper and not a subsystem
 *
 * Everything underneath already existed. `register_project` takes a folder, a
 * name and an optional custom command, and the custom command wins over
 * whatever the runner detected. So a shortcut is just a project registered at
 * the file's own folder with the command spelled out — no new runner, no new
 * table, no new launch path to keep in step with the existing one.
 *
 * What was missing was the front door. "Add a folder…" is the only entry
 * point, and nothing about it suggests you can point Launch Deck at
 * `pavlok-notify.mjs` or a `.ps1`, so nobody did.
 *
 * # The command is proposed, never assumed
 *
 * A `.ps1` wants PowerShell, a `.py` wants Python, and a `.docx` wants
 * whatever the shell opens it with. Guessing that silently would be the
 * `python main.py` mistake again — a confident command for a file nobody
 * checked. So the guess is filled into an editable field and shown before
 * saving. If it is wrong, it is wrong in front of you.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useMutation } from "@tanstack/react-query";
import { open as pickPath } from "@tauri-apps/plugin-dialog";
import { useState } from "react";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { api, asIpcError } from "@/lib/ipc";
import { useUi } from "@/state/ui";

/**
 * How to run a file, by extension.
 *
 * Only the interpreters actually present in this library get a guess. Anything
 * else is handed to the shell, which is what double-clicking it would do and
 * is the honest default for a file whose type we do not claim to understand.
 */
function proposeCommand(path: string): string {
  const quoted = path.includes(" ") ? `"${path}"` : path;
  const ext = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  switch (ext) {
    case "ps1":
      return `powershell.exe -NoProfile -ExecutionPolicy Bypass -File ${quoted}`;
    case "py":
      return `python ${quoted}`;
    case "js":
    case "mjs":
    case "cjs":
      return `node ${quoted}`;
    case "bat":
    case "cmd":
      return `cmd /c ${quoted}`;
    case "exe":
      return quoted;
    default:
      // `start` with an empty title, because the first quoted argument to
      // `start` is taken as the window title and a quoted path would be
      // swallowed by it.
      return `cmd /c start "" ${quoted}`;
  }
}

/** The folder a path lives in; a folder is its own. */
function parentOf(path: string, isDirectory: boolean): string {
  if (isDirectory) return path;
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut > 0 ? path.slice(0, cut) : path;
}

/** Last path segment, without its extension. */
function nameOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  const base = cut >= 0 ? path.slice(cut + 1) : path;
  const dot = base.lastIndexOf(".");
  return dot > 0 ? base.slice(0, dot) : base;
}

export function AddShortcutDialog() {
  const open = useUi((s) => s.shortcutDialogOpen);
  const close = useUi((s) => s.closeShortcutDialog);
  const toast = useUi((s) => s.toast);
  const select = useUi((s) => s.select);
  const queryClient = useQueryClient();

  const [target, setTarget] = useState("");
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [folder, setFolder] = useState("");
  const [error, setError] = useState<string | null>(null);

  const reset = () => {
    setTarget("");
    setName("");
    setCommand("");
    setFolder("");
    setError(null);
  };

  const choose = async (directory: boolean) => {
    try {
      const picked = await pickPath({
        directory,
        multiple: false,
        title: directory ? "Pick a folder to open" : "Pick a file or script",
      });
      if (typeof picked !== "string") return;
      setTarget(picked);
      setFolder(parentOf(picked, directory));
      setName(nameOf(picked));
      setCommand(
        directory
          ? `explorer.exe ${picked.includes(" ") ? `"${picked}"` : picked}`
          : proposeCommand(picked),
      );
      setError(null);
    } catch (e: unknown) {
      setError(asIpcError(e).message);
    }
  };

  const save = useMutation({
    mutationFn: () =>
      api.registerProject({
        // The folder is the working directory; the command names the file.
        path: folder,
        name: name.trim(),
        customCommand: command.trim(),
      }),
    onSuccess: (project) => {
      void queryClient.invalidateQueries({ queryKey: ["projects"] });
      select(project.id);
      toast("info", `Added ${project.name}`);
      reset();
      close();
    },
    onError: (e: unknown) => setError(asIpcError(e).message),
  });

  const submit = () => {
    if (!target) {
      setError("Pick a file or folder first.");
      return;
    }
    if (!name.trim()) {
      setError("Give it a name — that is what you will search for.");
      return;
    }
    if (!command.trim()) {
      setError("Say what should run when you launch it.");
      return;
    }
    setError(null);
    save.mutate();
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          reset();
          close();
        }
      }}
      title="Add a file, script or folder"
      description="Anything you want to launch that is not a whole project."
      width="lg"
    >
      <form
        className="flex flex-col gap-3 p-5"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <div className="flex gap-2">
          <Button type="button" variant="outline" onClick={() => void choose(false)}>
            Choose a file…
          </Button>
          <Button type="button" variant="outline" onClick={() => void choose(true)}>
            Choose a folder…
          </Button>
        </div>

        {target && (
          <p className="break-all rounded-control bg-fill-4 px-3 py-2 font-mono text-[12px] text-ink-strong">
            {target}
          </p>
        )}

        <label className="flex flex-col gap-1 text-xs text-ink/70">
          Name
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="pavlok-notify"
            aria-label="Name"
            aria-invalid={error?.includes("name") || undefined}
          />
        </label>

        <label className="flex flex-col gap-1 text-xs text-ink/70">
          Runs
          <Input
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder="node script.mjs"
            aria-label="Command"
            className="font-mono text-[13px]"
          />
          <span className="text-[11px] text-ink/70">
            {folder ? `in ${folder}` : "Proposed from the file type — change it if it is wrong."}
          </span>
        </label>

        {error && (
          <p id="shortcut-error" role="alert" className="text-xs text-signal">
            {error}
          </p>
        )}

        <div className="flex justify-end gap-2 pt-1">
          <Button
            type="button"
            onClick={() => {
              reset();
              close();
            }}
          >
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={save.isPending || !target}>
            {save.isPending ? "Adding…" : "Add"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
