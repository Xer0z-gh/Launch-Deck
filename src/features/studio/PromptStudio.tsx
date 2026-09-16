import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ClipboardCopy, FileDown, PenLine, SquareTerminal, X } from "lucide-react";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";

import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { api, asIpcError, type ProjectDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

import {
  assemblePrompt,
  TEMPLATE_BY_ID,
  TEMPLATES,
  type TemplateId,
} from "./templates";

/** A seed for repos that have no card yet — the sections a session needs. */
const CARD_SEED = `# LAUNCHDECK.md

## Stack

## Key paths & ports

## Conventions

## Current focus

## Known issues

## Do not touch
`;

/**
 * Prompt Studio: one sentence of intent + the repo's context card + a task
 * template = a complete prompt, previewed byte-for-byte and then copied,
 * handed off to NEXT.md, or loaded into a terminal with claude running.
 *
 * The preview IS the product — every action ships exactly the text shown,
 * so there is no "generate" step to wonder about.
 *
 * # Focus contract (mirrors the app's dialogs)
 *
 * Opening moves focus to the intent input; closing returns it to whatever
 * opened the studio, or to nothing rather than to a random control if the
 * opener unmounted. Esc peels ONE layer per press: card editor → studio.
 * The DetailBar underneath checks panel state before treating Esc as
 * "clear selection", so the layers never collapse together.
 */
export function PromptStudio({ project, open }: { project: ProjectDto; open: boolean }) {
  const close = useUi((s) => s.openStudio);
  const preset = useUi((s) => s.studioTemplate);
  const toast = useUi((s) => s.toast);
  const queryClient = useQueryClient();

  const [templateId, setTemplateId] = useState<TemplateId>(preset ?? "feature");
  const [intent, setIntent] = useState("");
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState<"terminal" | "handoff" | null>(null);

  const template = TEMPLATE_BY_ID[templateId];

  const openerRef = useRef<HTMLElement | null>(null);
  const intentRef = useRef<HTMLInputElement>(null);
  const editButtonRef = useRef<HTMLButtonElement>(null);
  const pillRefs = useRef(new Map<TemplateId, HTMLButtonElement>());

  const card = useQuery({
    queryKey: ["card", project.id],
    queryFn: () => api.contextCard(project.id),
  });

  // Errors are fetched once per open; the debug template embeds them and the
  // other templates ignore them, so one fetch serves every switch.
  const errors = useQuery({
    queryKey: ["errtail", project.id],
    queryFn: () => api.errorTail(project.id),
  });

  // Capture the opener and move focus in — the same contract a11y-verify
  // enforces on the dialogs. Runs once per mount (the panel is keyed by
  // project id, so a different project is a fresh mount).
  useEffect(() => {
    openerRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // The studio usually opens from a Radix menu item, and Radix restores
    // focus to its trigger when the menu closes -- which happens AFTER this
    // mount. A fixed one- or two-frame delay only wins that race on a
    // particular build's timing; it lost the moment the list was rebuilt.
    //
    // So this retries until it holds, briefly and with a hard stop. It gives
    // up the instant focus is already somewhere inside the studio, so it can
    // never fight a user who clicked in ahead of it.
    // Re-asserted across the WHOLE window, not until it first succeeds:
    // stopping on the first success is what kept losing, because Radix's
    // restore lands after that success and nothing took focus back.
    let frame = 0;
    let tries = 0;
    const claim = () => {
      const input = intentRef.current;
      const panel = input?.closest("aside");
      if (!input || !panel) return;
      // Focus already inside the panel -- either ours or the user's. Done.
      if (!panel.contains(document.activeElement)) input.focus();
      if (++tries < 30) frame = requestAnimationFrame(claim);
    };
    frame = requestAnimationFrame(claim);
    return () => cancelAnimationFrame(frame);
  }, []);

  const dismiss = () => {
    close(null);
    const opener = openerRef.current;
    if (opener?.isConnected) opener.focus();
  };

  const closeEditor = () => {
    setEditing(false);
    // The Edit-card button remounts on the next render; focus it then, so a
    // keyboard user resumes where the editor grew from instead of at <body>.
    requestAnimationFrame(() => editButtonRef.current?.focus());
  };

  // Esc peels one layer: editor first, then the studio. `preventDefault`
  // marks the event consumed so sibling listeners (DetailBar) can tell a
  // handled Esc from a free one.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      if (editing) closeEditor();
      else dismiss();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editing]);

  // Roving tabindex for the template radios: one Tab stop, arrows move the
  // selection — the behavior the radio role promises.
  const onPillKeyDown = (e: ReactKeyboardEvent, index: number) => {
    const delta =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    let next: number | null = null;
    if (delta !== 0) next = (index + delta + TEMPLATES.length) % TEMPLATES.length;
    if (e.key === "Home") next = 0;
    if (e.key === "End") next = TEMPLATES.length - 1;
    if (next === null) return;
    e.preventDefault();
    const target = TEMPLATES[next];
    if (!target) return;
    setTemplateId(target.id);
    pillRefs.current.get(target.id)?.focus();
  };

  const prompt = useMemo(
    () =>
      assemblePrompt({
        project,
        template,
        intent,
        card: card.data,
        errors: errors.data ?? [],
      }),
    [project, template, intent, card.data, errors.data],
  );

  const copyPrompt = () => {
    void navigator.clipboard
      .writeText(prompt)
      .then(() => toast("info", "Prompt copied"))
      .catch(() => toast("error", "Clipboard unavailable"));
  };

  const openTerminal = async () => {
    setBusy("terminal");
    try {
      await api.openClaudeTerminal(project.id, prompt);
      toast("info", "Terminal opened — prompt loaded and on the clipboard");
    } catch (e) {
      toast("error", asIpcError(e).message);
    } finally {
      setBusy(null);
    }
  };

  const handOff = async () => {
    setBusy("handoff");
    try {
      const path = await api.handOff(project.id, prompt);
      toast("info", `Appended to ${path}`);
    } catch (e) {
      toast("error", asIpcError(e).message);
    } finally {
      setBusy(null);
    }
  };

  const saveCard = async () => {
    try {
      await api.saveContextCard(project.id, draft);
      closeEditor();
      await queryClient.invalidateQueries({ queryKey: ["card", project.id] });
      toast("info", "Context card saved");
    } catch (e) {
      toast("error", asIpcError(e).message);
    }
  };

  const tails = errors.data ?? [];
  const matchedCount = tails.filter((t) => t.matched).reduce((n, t) => n + t.lines.length, 0);
  const rawCount = tails.filter((t) => !t.matched).reduce((n, t) => n + t.lines.length, 0);

  return (
    <aside
      aria-label={`Prompt Studio for ${project.name}`}
      style={{ "--panel-w": "40%", "--panel-min": "380px" } as CSSProperties}
      className={cn(
        "panel-enter flex h-full max-w-[880px] shrink-0 flex-col overflow-hidden border-l border-rule bg-panel",
        open && "is-open",
      )}
    >
      <header className="flex items-center gap-2 border-b border-rule px-4 py-2.5">
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-[13px] font-semibold text-ink">
            {project.name}
            <span className="ml-2 font-normal text-ink/45">Prompt Studio</span>
          </h2>
          <p className="truncate text-[11px] text-ink/60">{template.hint}</p>
        </div>
        <Tooltip label="Close">
          <Button size="icon" aria-label="Close Prompt Studio" onClick={dismiss}>
            <X size={13} />
          </Button>
        </Tooltip>
      </header>

      <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
        {/* Template picker: real radio semantics, one Tab stop, arrows move. */}
        <div role="radiogroup" aria-label="Task template" className="flex flex-wrap gap-1.5">
          {TEMPLATES.map((t, i) => (
            <button
              key={t.id}
              ref={(el) => {
                if (el) pillRefs.current.set(t.id, el);
                else pillRefs.current.delete(t.id);
              }}
              type="button"
              role="radio"
              aria-checked={t.id === templateId}
              tabIndex={t.id === templateId ? 0 : -1}
              onClick={() => setTemplateId(t.id)}
              onKeyDown={(e) => onPillKeyDown(e, i)}
              className={cn(
                "rounded-control border px-2.5 py-1 text-xs",
                "transition-colors duration-[120ms] ease-[var(--ease-standard)]",
                "focus-visible:outline-2 focus-visible:outline-offset-0 focus-visible:outline-accent",
                t.id === templateId
                  ? "border-rule bg-raised text-ink"
                  : "border-transparent text-ink/60 hover:bg-raised/60 hover:text-ink/85",
              )}
            >
              {t.label}
            </button>
          ))}
        </div>

        <label className="flex flex-col gap-1">
          <span className="text-[11px] uppercase tracking-wide text-ink/60">
            Intent — one sentence
          </span>
          <Input
            ref={intentRef}
            value={intent}
            onChange={(e) => setIntent(e.target.value)}
            placeholder={
              templateId === "debug"
                ? "What looks broken?"
                : "What should this session accomplish?"
            }
          />
        </label>

        {/* Context card: state + edit in place. */}
        <div className="rounded-control border border-rule bg-field">
          <div className="flex items-center gap-2 px-3 py-2">
            <span className="min-w-0 flex-1 truncate text-xs text-ink/70">
              {card.data?.exists
                ? `LAUNCHDECK.md · ${card.data.content.split("\n").length} lines`
                : "No context card yet"}
            </span>
            {!editing && (
              <Button
                ref={editButtonRef}
                size="sm"
                variant="outline"
                onClick={() => {
                  setDraft(card.data?.exists ? card.data.content : CARD_SEED);
                  setEditing(true);
                }}
              >
                <PenLine size={12} />
                {card.data?.exists ? "Edit card" : "Create card"}
              </Button>
            )}
          </div>
          {editing && (
            <div className="flex flex-col gap-2 border-t border-rule p-3">
              <textarea
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={(e) => {
                  // Esc backs out of the editor only — stopPropagation keeps
                  // the studio's own Esc handler from also firing this press.
                  if (e.key === "Escape") {
                    e.preventDefault();
                    e.stopPropagation();
                    closeEditor();
                  }
                }}
                aria-label="Context card content"
                rows={12}
                spellCheck={false}
                // Deliberate: the editor just grew from the Edit-card button
                // the user activated; focusing its textarea IS the expected
                // continuation, not a focus theft.
                autoFocus
                className={cn(
                  "w-full resize-y rounded-control border border-rule bg-panel p-2.5",
                  "font-mono text-xs leading-relaxed text-ink",
                  "focus-visible:border-accent focus-visible:outline-2",
                  "focus-visible:outline-offset-0 focus-visible:outline-accent",
                )}
              />
              <div className="flex justify-end gap-1.5">
                <Button size="sm" onClick={closeEditor}>
                  Cancel
                </Button>
                <Button size="sm" variant="primary" onClick={() => void saveCard()}>
                  Save card
                </Button>
              </div>
            </div>
          )}
        </div>

        {/* Tail note: only when the chosen template embeds logs, and never
            calling a raw tail "errors" — that distinction comes from the
            backend's matched flag and survives into the prompt itself. */}
        {template.wantsErrors && (
          <p className="text-[11px] text-ink/60">
            {errors.isLoading
              ? "Reading recent logs…"
              : matchedCount > 0
                ? `${matchedCount} error line${matchedCount === 1 ? "" : "s"} embedded from ${tails
                    .filter((t) => t.matched)
                    .map((t) => t.source)
                    .join(", ")}${
                    rawCount > 0
                      ? `, plus ${rawCount} plain log line${rawCount === 1 ? "" : "s"} from ${tails
                          .filter((t) => !t.matched)
                          .map((t) => t.source)
                          .join(", ")}`
                      : ""
                  }`
                : rawCount > 0
                  ? `No error-marked lines found — embedding the last ${rawCount} log lines from ${tails
                      .filter((t) => !t.matched)
                      .map((t) => t.source)
                      .join(", ")} instead`
                  : "No recent logs found for this project — the prompt says so honestly."}
          </p>
        )}

        {/* The product: exactly what every action ships. Focusable so a
            keyboard user can scroll the whole text, not just the first
            viewport of it. */}
        <div className="flex min-h-[160px] flex-1 flex-col rounded-control border border-rule bg-field">
          <div className="border-b border-rule px-3 py-1.5 text-[11px] uppercase tracking-wide text-ink/60">
            Prompt · {prompt.length.toLocaleString()} chars
          </div>
          <pre
            tabIndex={0}
            role="region"
            aria-label="Prompt preview"
            className={cn(
              "min-h-0 flex-1 overflow-auto whitespace-pre-wrap p-3 font-mono text-[11px] leading-relaxed text-ink/80",
              "focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent",
            )}
          >
            {prompt}
          </pre>
        </div>
      </div>

      <footer className="flex items-center gap-1.5 border-t border-rule px-4 py-3">
        <Button
          variant="primary"
          size="md"
          disabled={busy !== null}
          onClick={() => void openTerminal()}
        >
          <SquareTerminal size={13} />
          {busy === "terminal" ? "Opening…" : "Open in terminal"}
        </Button>
        <Button size="md" variant="outline" onClick={copyPrompt}>
          <ClipboardCopy size={13} />
          Copy
        </Button>
        <Tooltip label={`Append to NEXT.md in ${project.root}`}>
          <Button
            size="md"
            variant="outline"
            disabled={busy !== null}
            onClick={() => void handOff()}
          >
            <FileDown size={13} />
            {busy === "handoff" ? "Writing…" : "NEXT.md"}
          </Button>
        </Tooltip>
      </footer>
    </aside>
  );
}
