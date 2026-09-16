/**
 * Wires a project's status source: the URL, port, and log file its tile
 * watches beyond the supervisor.
 *
 * Three independent fields, all optional — a deployed site has only a URL, a
 * panel has a URL and a port, an ops script may have only a log. Saving with
 * everything empty clears the source (the backend normalises an all-empty
 * config to none), so "stop watching this" needs no separate destructive
 * control.
 */
import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { cn } from "@/lib/cn";
import { asIpcError, type FactsSource, type ProjectDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

import { useUpdateProject } from "./useProjects";

export function SurfaceDialog({
  project,
  open,
  onOpenChange,
}: {
  project: ProjectDto;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const update = useUpdateProject();
  const toast = useUi((s) => s.toast);
  const queryClient = useQueryClient();

  const [url, setUrl] = useState(project.surface?.url ?? "");
  const [port, setPort] = useState(project.surface?.probePort?.toString() ?? "");
  const [log, setLog] = useState(project.surface?.externalLog ?? "");
  const [facts, setFacts] = useState<FactsSource | "">(project.surface?.facts ?? "");
  const [error, setError] = useState<string | null>(null);

  const save = () => {
    const trimmedUrl = url.trim();
    const trimmedLog = log.trim();
    const trimmedPort = port.trim();

    // Mirror the backend's scheme gate here so the mistake is caught where it
    // was typed, with the field still focused, rather than as a failed save.
    if (trimmedUrl && !/^https?:\/\//.test(trimmedUrl)) {
      setError("URL must start with http:// or https://");
      return;
    }
    let probePort: number | null = null;
    if (trimmedPort) {
      const n = Number(trimmedPort);
      if (!Number.isInteger(n) || n < 1 || n > 65535) {
        setError("Port must be a whole number between 1 and 65535");
        return;
      }
      probePort = n;
    }
    setError(null);

    const empty = !trimmedUrl && !probePort && !trimmedLog;
    update.mutate(
      {
        id: project.id,
        patch: {
          surface: empty
            ? null
            : {
                url: trimmedUrl || null,
                probePort,
                externalLog: trimmedLog || null,
                facts: facts || null,
              },
        },
      },
      {
        onSuccess: () => {
          // The reading for the old config is stale the moment the config
          // changes; drop it so the chip re-probes instead of showing the
          // previous source's numbers under the new source's name.
          void queryClient.invalidateQueries({ queryKey: ["surface", project.id] });
          onOpenChange(false);
        },
        onError: (e: unknown) => toast("error", asIpcError(e).message),
      },
    );
  };

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Status source"
      description={`What ${project.name}'s tile watches beyond the supervisor. Leave every field empty to stop watching.`}
    >
      <form
        className="flex flex-col gap-3 p-5"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <label className="flex flex-col gap-1 text-xs text-ink/60">
          URL to ping and open
          <Input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://vendsuite.vercel.app"
            aria-label="URL to ping and open"
            aria-describedby={error ? "surface-error" : undefined}
          />
        </label>
        <label className="flex flex-col gap-1 text-xs text-ink/60">
          Local port to probe
          <Input
            value={port}
            onChange={(e) => setPort(e.target.value)}
            placeholder="4173"
            inputMode="numeric"
            aria-label="Local port to probe"
            aria-describedby={error ? "surface-error" : undefined}
          />
        </label>
        <label className="flex flex-col gap-1 text-xs text-ink/60">
          Log file to watch
          <Input
            value={log}
            onChange={(e) => setLog(e.target.value)}
            placeholder="D:\\Workspace\\Dev\\Rust\\Fleet\\data\\fleet.log"
            aria-label="Log file to watch"
          />
        </label>

        {/* The reader is what turns "it answered" into "here is what it said".
            Listed by app name rather than by endpoint: the adapter knows which
            endpoints to call, and naming them here would go stale the first
            time one of those apps moves a route. */}
        <label className="flex flex-col gap-1 text-xs text-ink/60">
          Read facts from
          <select
            value={facts}
            onChange={(e) => setFacts(e.target.value as FactsSource | "")}
            aria-label="Read facts from"
            className={cn(
              "h-[34px] w-full rounded-capsule border-0 bg-fill-3 px-3.5",
              "text-[15px] text-ink-strong",
              "transition-colors duration-[140ms] ease-[var(--ease-standard)]",
              "hover:bg-fill-2 focus-visible:bg-fill-2",
              "focus-visible:outline-2 focus-visible:outline-offset-0 focus-visible:outline-accent",
            )}
          >
            <option value="">Reachability only</option>
            <option value="fleet">Fleet — active runs, armed teams, plan window</option>
            <option value="ollama">Ollama — models installed and loaded</option>
            <option value="lathe">Lathe — Ollama health, models, request count</option>
            <option value="focus-forge">FocusForge — play credit, flow, priority gate</option>
            <option value="pavlok-status">Pavlok — last alert from the status file</option>
          </select>
          <span className="text-[11px] text-ink/45">
            {facts === "pavlok-status"
              ? "Reads the log file above."
              : facts
                ? "Reads its own endpoints on the URL's origin."
                : "The tile shows only whether the source answers."}
          </span>
        </label>

        {error && (
          <p id="surface-error" role="alert" className="text-xs text-signal">
            {error}
          </p>
        )}

        <div className="flex justify-end gap-2 pt-1">
          <Button type="button" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={update.isPending}>
            Save
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
