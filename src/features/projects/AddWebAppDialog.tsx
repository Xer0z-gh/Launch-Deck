/**
 * Saves a URL as a launchable project.
 *
 * The point is instant access: the deployed sites, hosted panels and
 * dashboards that are part of the daily set of "my apps" but have no folder on
 * this machine. Once saved, one lives in the library beside everything else --
 * searchable, favouritable, watched by the same status probe, and opened in a
 * window inside Launch Deck rather than in a browser tab that gets lost.
 *
 * What it creates is a real artefact: a folder holding a Windows `.url`
 * shortcut. That is deliberate. An entry that exists only as a database row
 * disappears with the database; a shortcut survives, opens from Explorer, and
 * is re-detected like any other project.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useMutation } from "@tanstack/react-query";
import { useRef, useState } from "react";

import { Button } from "@/components/ui/Button";
import { Dialog } from "@/components/ui/Dialog";
import { Input } from "@/components/ui/Input";
import { api, asIpcError } from "@/lib/ipc";
import { useUi } from "@/state/ui";

export function AddWebAppDialog() {
  const open = useUi((s) => s.webDialogOpen);
  const close = useUi((s) => s.closeWebDialog);
  const toast = useUi((s) => s.toast);
  const select = useUi((s) => s.select);
  const queryClient = useQueryClient();

  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  /**
   * Which field the message is about, not just the message.
   *
   * Both inputs used to point `aria-describedby` at one shared node, so a
   * screen-reader user on the Address field heard "Give it a name" -- an error
   * about a different field -- and nothing carried `aria-invalid` to say which
   * one was wrong.
   */
  const [error, setError] = useState<{ field: "name" | "url"; message: string } | null>(null);

  const nameRef = useRef<HTMLInputElement>(null);
  const urlRef = useRef<HTMLInputElement>(null);

  const save = useMutation({
    mutationFn: () => api.registerWebApp(name.trim(), url.trim()),
    onSuccess: (project) => {
      void queryClient.invalidateQueries({ queryKey: ["projects"] });
      select(project.id);
      toast("info", `Saved ${project.name}. It is in the library now.`);
      reset();
      close();
    },
    // A backend rejection is about the address: the name is unvalidated
    // there, and every refusal it can return names the URL.
    onError: (e: unknown) => {
      setError({ field: "url", message: asIpcError(e).message });
      urlRef.current?.focus();
    },
  });

  const reset = () => {
    setName("");
    setUrl("");
    setError(null);
  };

  const submit = () => {
    const trimmedUrl = url.trim();
    const trimmedName = name.trim();
    if (!trimmedName) {
      setError({ field: "name", message: "Give it a name — that is what you will search for." });
      // Focus follows the error, so the fix is one keystroke away rather than
      // a hunt for which field the message belongs to.
      nameRef.current?.focus();
      return;
    }
    // The same scheme gate the backend applies, checked here so the mistake
    // is caught with the field still focused rather than as a failed save.
    if (!/^https?:\/\//.test(trimmedUrl)) {
      setError({ field: "url", message: "The address must start with http:// or https://" });
      urlRef.current?.focus();
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
      title="Add a web app"
      description="A deployed site or hosted panel, saved so it opens in a window here."
    >
      <form
        className="flex flex-col gap-3 p-5"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <label className="flex flex-col gap-1 text-xs text-ink/60">
          Name
          <Input
            ref={nameRef}
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Vendsuite"
            aria-label="Name"
            aria-invalid={error?.field === "name" || undefined}
            aria-describedby={error?.field === "name" ? "web-error" : undefined}
            autoFocus
          />
        </label>

        <label className="flex flex-col gap-1 text-xs text-ink/60">
          Address
          <Input
            ref={urlRef}
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://vendsuite.vercel.app"
            aria-label="Address"
            aria-invalid={error?.field === "url" || undefined}
            aria-describedby={error?.field === "url" ? "web-error" : undefined}
            inputMode="url"
          />
        </label>

        {error && (
          <p id="web-error" role="alert" className="text-xs text-signal">
            {error.message}
          </p>
        )}

        <p className="text-[11px] leading-relaxed text-ink/70">
          Launch Deck will watch this address and show whether it answers, the
          same as any other status source.
        </p>

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
          <Button type="submit" variant="primary" disabled={save.isPending}>
            {save.isPending ? "Saving…" : "Save"}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
