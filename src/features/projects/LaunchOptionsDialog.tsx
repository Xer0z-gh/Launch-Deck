/**
 * What Run will actually execute, and everything you can change about it.
 *
 * # What this replaces
 *
 * A single text box holding the whole command. That covered "change it" and
 * nothing else: to drop `--port 4000` for an afternoon you deleted it and
 * retyped it later, which is where the typo comes from. Flags are the part
 * that changes; the command underneath usually is not.
 *
 * So the two are separated. The command is one field. The flags are rows you
 * can switch off and back on without losing them. And the line that matters --
 * the one that will really run -- is assembled from both and shown at the top,
 * because a launcher that cannot tell you what it is about to do is asking to
 * be trusted rather than read.
 *
 * # Why the preview is built here rather than fetched
 *
 * It is `command + enabled flags`, which is the whole rule, and computing it
 * locally means it updates as you type instead of after a round trip. The
 * backend composes the real thing the same way (`plan()` appends
 * `overrides.args` for long-running steps), and the row's own `runCommand`
 * — which DOES come from the backend — is shown beside it whenever the two
 * disagree, so a drift between them is visible rather than silent.
 */
import { useState } from "react";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { cn } from "@/lib/cn";
import { asIpcError, type ProjectDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

import { useUpdateProject } from "./useProjects";

/** One flag, with the state the dialog is editing. */
interface Flag {
  value: string;
  enabled: boolean;
}

export function LaunchOptionsDialog({
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

  // Seeded once per opening: `key` on the caller remounts this when the
  // project changes, so there is no stale-draft problem to guard against.
  const [command, setCommand] = useState(project.runCommand ?? "");
  const [flags, setFlags] = useState<Flag[]>(() => [
    ...project.args.map((value) => ({ value, enabled: true })),
    ...project.disabledArgs.map((value) => ({ value, enabled: false })),
  ]);
  const [draft, setDraft] = useState("");

  const addFlag = () => {
    const value = draft.trim();
    if (!value) return;
    setFlags((f) => [...f, { value, enabled: true }]);
    setDraft("");
  };

  const preview = [command.trim(), ...flags.filter((f) => f.enabled).map((f) => f.value)]
    .filter(Boolean)
    .join(" ");

  const save = () => {
    const next = command.trim();
    update.mutate(
      {
        id: project.id,
        patch: {
          // `null` clears the override so the runner's own command returns.
          runCommand: next.length > 0 ? next : null,
          args: flags.filter((f) => f.enabled).map((f) => f.value),
          disabledArgs: flags.filter((f) => !f.enabled).map((f) => f.value),
        },
      },
      {
        onSuccess: () => onOpenChange(false),
        onError: (e: unknown) => toast("error", asIpcError(e).message),
      },
    );
  };

  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="Launch options"
      description={`What Run executes for ${project.name}.`}
      width="lg"
    >
      <form
        className="flex flex-col gap-4 p-5"
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        {/* The answer to "what will actually run", first, because it is the
            question. Everything below it is how to change it. */}
        <section aria-labelledby="lo-preview" className="flex flex-col gap-1.5">
          <h3 id="lo-preview" className="text-[13px] font-semibold text-ink/70">
            Will run
          </h3>
          <p className="break-all rounded-control bg-fill-4 px-3 py-2 font-mono text-[12px] leading-relaxed text-ink-strong">
            {preview || <span className="text-ink/60">nothing — no command set</span>}
          </p>
          <p className="text-[11px] text-ink/70">
            in {project.root}
          </p>
        </section>

        <label className="flex flex-col gap-1 text-xs text-ink/70">
          Command
          <Input
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder={project.runCommand ?? "npm run dev"}
            aria-label="Command"
            className="font-mono text-[13px]"
          />
          <span className="text-[11px] text-ink/70">
            Leave empty to go back to whatever the {project.kindLabel} runner
            detected. This is not a shell — pipes and redirects are passed
            through as arguments.
          </span>
        </label>

        <section aria-labelledby="lo-flags" className="flex flex-col gap-2">
          <h3 id="lo-flags" className="text-[13px] font-semibold text-ink/70">
            Flags
          </h3>

          {flags.length === 0 ? (
            <p className="text-[13px] text-ink/70">
              None. Anything added here is appended to the command above.
            </p>
          ) : (
            <ul className="flex flex-col overflow-hidden rounded-control bg-fill-4">
              {flags.map((flag, i) => (
                <li
                  key={`${flag.value}-${i}`}
                  className="flex items-center gap-2 border-b border-rule/60 px-2 py-1.5 last:border-b-0"
                >
                  {/* A real checkbox: it is a two-state control with a label,
                      which is what a checkbox is for, and it gets keyboard
                      and screen-reader behaviour without any help. */}
                  <input
                    type="checkbox"
                    id={`flag-${i}`}
                    checked={flag.enabled}
                    onChange={(e) =>
                      setFlags((f) =>
                        f.map((x, j) => (j === i ? { ...x, enabled: e.target.checked } : x)),
                      )
                    }
                    className="h-4 w-4 shrink-0 accent-[var(--color-accent)]"
                  />
                  <label
                    htmlFor={`flag-${i}`}
                    className={cn(
                      "min-w-0 flex-1 cursor-pointer truncate font-mono text-[12px]",
                      flag.enabled ? "text-ink-strong" : "text-ink/60 line-through",
                    )}
                  >
                    {flag.value}
                  </label>
                  <Button
                    size="sm"
                    variant="danger"
                    aria-label={`Remove ${flag.value}`}
                    onClick={() => setFlags((f) => f.filter((_, j) => j !== i))}
                  >
                    Remove
                  </Button>
                </li>
              ))}
            </ul>
          )}

          <div className="flex gap-2">
            <Input
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="--port 4000"
              aria-label="New flag"
              className="font-mono text-[13px]"
              onKeyDown={(e) => {
                // Enter adds the flag rather than submitting the form, which
                // would save and close with the flag still in the box.
                if (e.key === "Enter") {
                  e.preventDefault();
                  addFlag();
                }
              }}
            />
            <Button onClick={addFlag} disabled={!draft.trim()}>
              Add
            </Button>
          </div>
        </section>

        <div className="flex justify-end gap-2 pt-1">
          <Button type="button" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button type="submit" variant="primary" disabled={update.isPending}>
            {update.isPending ? "Saving…" : "Save"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
