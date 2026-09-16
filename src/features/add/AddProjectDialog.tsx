import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { FolderSearch } from "lucide-react";
import { useEffect, useState } from "react";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { api, asIpcError, type InspectDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

/**
 * The add flow: pick or drop a folder → see exactly what was detected and the
 * exact command Run would execute → confirm. Detection proposes; the user
 * decides. Nothing runs until they later press Run.
 */
export function AddProjectDialog() {
  const open = useUi((s) => s.addDialogOpen);
  const droppedPath = useUi((s) => s.addDialogPath);
  const close = useUi((s) => s.closeAddDialog);
  const toast = useUi((s) => s.toast);
  const queryClient = useQueryClient();

  const [inspecting, setInspecting] = useState(false);
  const [inspection, setInspection] = useState<InspectDto | null>(null);
  const [name, setName] = useState("");
  const [runnerId, setRunnerId] = useState<string | null>(null);
  const [customCommand, setCustomCommand] = useState("");
  const [registering, setRegistering] = useState(false);

  const reset = () => {
    setInspection(null);
    setName("");
    setRunnerId(null);
    setCustomCommand("");
  };

  const inspect = async (path: string) => {
    setInspecting(true);
    try {
      const result = await api.inspectPath(path);
      setInspection(result);
      setName(result.name);
      setRunnerId(result.matches[0]?.runnerId ?? null);
    } catch (e) {
      toast("error", asIpcError(e).message);
      close();
    } finally {
      setInspecting(false);
    }
  };

  // A path dropped onto the window arrives pre-filled.
  useEffect(() => {
    if (open && droppedPath) void inspect(droppedPath);
    if (!open) reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, droppedPath]);

  const choose = async () => {
    const path = await pickFolder({ directory: true, title: "Add a project" });
    if (typeof path === "string") await inspect(path);
  };

  const register = async () => {
    if (!inspection) return;
    setRegistering(true);
    try {
      await api.registerProject({
        path: inspection.root,
        name: name.trim() || inspection.name,
        ...(runnerId ? { runnerId } : {}),
        ...(customCommand.trim() ? { customCommand: customCommand.trim() } : {}),
      });
      await queryClient.invalidateQueries({ queryKey: ["projects"] });
      close();
    } catch (e) {
      toast("error", asIpcError(e).message);
    } finally {
      setRegistering(false);
    }
  };

  const selected = inspection?.matches.find((m) => m.runnerId === runnerId);
  const unidentified = inspection !== null && inspection.matches.length === 0;
  const canRegister =
    inspection !== null &&
    !inspection.alreadyRegistered &&
    (!unidentified || customCommand.trim().length > 0);

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) close();
      }}
      title="Add project"
      description="Point at a folder; Launch Deck works out what it is and how to run it."
    >
      {inspection === null ? (
        <div className="flex flex-col items-center gap-3 py-6">
          <FolderSearch size={28} className="text-ink/30" />
          <p className="text-xs text-ink/55">
            Choose a project folder, or drop one anywhere on the window.
          </p>
          <Button variant="primary" onClick={() => void choose()} disabled={inspecting}>
            {inspecting ? "Inspecting…" : "Choose folder…"}
          </Button>
        </div>
      ) : (
        <div className="flex flex-col gap-4">
          <div>
            <label className="mb-1 block text-[11px] uppercase tracking-wide text-ink/40">
              Name
            </label>
            <Input
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-label="Project name"
            />
            <p className="mt-1 truncate text-[11px] text-ink/35">{inspection.root}</p>
          </div>

          {inspection.alreadyRegistered && (
            <p className="rounded-control border border-warn/40 bg-warn/10 px-3 py-2 text-xs text-warn">
              This folder is already registered.
            </p>
          )}

          {unidentified ? (
            <div>
              <label className="mb-1 block text-[11px] uppercase tracking-wide text-ink/40">
                Run command
              </label>
              <Input
                value={customCommand}
                onChange={(e) => setCustomCommand(e.target.value)}
                placeholder={'e.g.  python main.py  or  "C:\\Tools\\serve.exe" --port 8080'}
                aria-label="Custom run command"
              />
              <p className="mt-1 text-[11px] text-ink/45">
                No project type was identified, so tell Launch Deck what Run should
                execute. Quotes group arguments; nothing is passed to a shell.
              </p>
            </div>
          ) : (
            <div>
              <label className="mb-1 block text-[11px] uppercase tracking-wide text-ink/40">
                Detected as
              </label>
              <div className="flex flex-col gap-1" role="radiogroup" aria-label="Project type">
                {inspection.matches.map((m) => (
                  <label
                    key={m.runnerId}
                    className="flex cursor-pointer items-center gap-2.5 rounded-control border border-rule px-3 py-2 text-xs has-[:checked]:border-accent has-[:checked]:bg-accent/5"
                  >
                    <input
                      type="radio"
                      name="runner"
                      className="accent-accent"
                      checked={runnerId === m.runnerId}
                      onChange={() => setRunnerId(m.runnerId)}
                    />
                    <span className="font-medium text-ink">{m.runnerName}</span>
                    <span className="text-ink/45">
                      {m.framework ?? m.language}
                      {m.version ? ` · v${m.version}` : ""}
                    </span>
                  </label>
                ))}
              </div>
              {selected?.proposedRun && (
                <p className="mt-2 text-[11px] text-ink/45">
                  Run will execute{" "}
                  <code className="rounded bg-field px-1.5 py-0.5 font-mono text-[11px] text-ink/80">
                    {selected.proposedRun}
                  </code>
                </p>
              )}
            </div>
          )}

          <div className="flex justify-end gap-2 border-t border-rule pt-3">
            <Button onClick={close}>Cancel</Button>
            <Button
              variant="primary"
              disabled={!canRegister || registering}
              onClick={() => void register()}
            >
              {registering ? "Adding…" : "Add project"}
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  );
}
