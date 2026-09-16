import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { api, asIpcError, type ScanResultDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

/**
 * Bulk discovery: scan a workspace, review what was found, register the
 * selection. Already-registered folders are shown but not re-registerable, and
 * a truncated scan says so rather than looking complete.
 */
export function ScanDialog() {
  const open = useUi((s) => s.scanDialogOpen);
  const close = useUi((s) => s.closeScanDialog);
  const toast = useUi((s) => s.toast);
  const queryClient = useQueryClient();

  const [scanning, setScanning] = useState(false);
  const [result, setResult] = useState<ScanResultDto | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [registering, setRegistering] = useState(false);

  const reset = () => {
    setResult(null);
    setSelected(new Set());
  };

  const scan = async () => {
    const path = await pickFolder({ directory: true, title: "Scan a folder for projects" });
    if (typeof path !== "string") return;
    setScanning(true);
    try {
      const found = await api.scanPath(path);
      setResult(found);
      setSelected(
        new Set(found.hits.filter((h) => !h.alreadyRegistered).map((h) => h.root)),
      );
    } catch (e) {
      toast("error", asIpcError(e).message);
    } finally {
      setScanning(false);
    }
  };

  const registerSelected = async () => {
    if (!result) return;
    setRegistering(true);
    let added = 0;
    let failed = 0;
    for (const hit of result.hits) {
      if (!selected.has(hit.root) || hit.alreadyRegistered) continue;
      try {
        await api.registerProject({ path: hit.root, name: hit.name });
        added += 1;
      } catch {
        failed += 1;
      }
    }
    await queryClient.invalidateQueries({ queryKey: ["projects"] });
    setRegistering(false);
    if (failed > 0) toast("error", `${failed} project(s) could not be added`);
    if (added > 0) toast("info", `Added ${added} project(s)`);
    close();
    reset();
  };

  const toggle = (root: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(root)) next.delete(root);
      else next.add(root);
      return next;
    });
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          close();
          reset();
        }
      }}
      title="Scan for projects"
      description="Walks a folder tree (skipping node_modules, target, .git…) and lists everything it can identify."
      width="lg"
    >
      {result === null ? (
        <div className="flex flex-col items-center gap-3 py-6">
          <p className="text-xs text-ink/55">
            Point at a workspace root — for example{" "}
            <code className="rounded bg-field px-1.5 py-0.5 font-mono text-[11px]">
              D:\Workspace\Dev
            </code>
          </p>
          <Button variant="primary" onClick={() => void scan()} disabled={scanning}>
            {scanning ? "Scanning…" : "Choose folder to scan…"}
          </Button>
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <p className="text-[11px] text-ink/45">
            {result.hits.length} project(s) found across {result.directoriesVisited}{" "}
            directories.
            {result.truncated && (
              <span className="text-warn"> Result capped — narrow the folder.</span>
            )}
          </p>

          <div className="max-h-80 overflow-y-auto rounded-panel border border-rule">
            {result.hits.length === 0 ? (
              <p className="px-4 py-6 text-center text-xs text-ink/45">
                Nothing identifiable here. Add a single folder instead to type a
                custom run command.
              </p>
            ) : (
              result.hits.map((hit) => (
                <label
                  key={hit.root}
                  className="flex cursor-pointer items-center gap-3 border-b border-rule/50 px-3 py-2 text-xs last:border-b-0 hover:bg-raised"
                >
                  <input
                    type="checkbox"
                    className="accent-accent"
                    disabled={hit.alreadyRegistered}
                    checked={!hit.alreadyRegistered && selected.has(hit.root)}
                    onChange={() => toggle(hit.root)}
                  />
                  <span className="w-40 truncate font-medium text-ink">{hit.name}</span>
                  <span className="w-24 shrink-0 text-ink/50">{hit.kindLabel ?? "—"}</span>
                  <span className="min-w-0 flex-1 truncate text-ink/35" dir="rtl">
                    {hit.root}
                  </span>
                  {hit.alreadyRegistered && (
                    <span className="shrink-0 text-[10px] uppercase tracking-wide text-ink/35">
                      added
                    </span>
                  )}
                </label>
              ))
            )}
          </div>

          <div className="flex justify-between gap-2 border-t border-rule pt-3">
            <Button onClick={() => void scan()}>Scan another folder…</Button>
            <div className="flex gap-2">
              <Button
                onClick={() => {
                  close();
                  reset();
                }}
              >
                Cancel
              </Button>
              <Button
                variant="primary"
                disabled={registering || selected.size === 0}
                onClick={() => void registerSelected()}
              >
                {registering ? "Adding…" : `Add ${selected.size} selected`}
              </Button>
            </div>
          </div>
        </div>
      )}
    </Dialog>
  );
}
