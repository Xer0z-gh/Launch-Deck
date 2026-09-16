/**
 * Prompt Studio's task templates and the assembler that turns
 * (project, card, intent, errors) into one complete, paste-ready prompt.
 *
 * Assembly is pure text and happens here, not in the backend: there is no
 * decision in it, only concatenation, and keeping it client-side means the
 * preview IS the output — what the studio shows is byte-for-byte what gets
 * handed off, copied, or loaded into the terminal.
 */

import type { ErrorTailDto, ProjectDto } from "@/lib/ipc";

export type TemplateId =
  | "debug"
  | "feature"
  | "refactor"
  | "design"
  | "copy"
  | "ship";

export interface Template {
  id: TemplateId;
  label: string;
  /** One line under the picker saying what this template is for. */
  hint: string;
  /**
   * Task-specific instructions; `intent` is the user's one sentence.
   * `hasMatchedErrors` is true only when the embedded tail actually carried
   * error-marked lines — a template must not point at "the errors above"
   * when the tail is a plain raw fallback.
   */
  body: (intent: string, hasMatchedErrors: boolean) => string;
  /** Whether the assembled prompt should carry the recent-error tail. */
  wantsErrors: boolean;
}

export const TEMPLATES: Template[] = [
  {
    id: "debug",
    label: "Debug",
    hint: "Something is broken; find the root cause and fix it once.",
    wantsErrors: true,
    body: (intent, hasMatchedErrors) => `## Task

${
  intent ||
  (hasMatchedErrors
    ? "Something is wrong — the symptom is in the errors above."
    : "Something is wrong, but recent logs carry no error-marked lines — start by reproducing the symptom.")
}

Reproduce the failure before changing anything. Then find the root cause —
the fix goes where every affected caller routes through, not where the
symptom happened to surface. Smallest change that actually fixes it, plus
one regression check that fails without the fix. Report honestly what was
proven by running something versus what is still assumption.`,
  },
  {
    id: "feature",
    label: "Feature",
    hint: "Add a capability without breaking the shape of the app.",
    wantsErrors: false,
    body: (intent) => `## Task

${intent || "Build the feature described above."}

Read the surrounding code first and follow its patterns — this lands as one
coherent slice, not a parallel system. Production bar applies: loading,
empty and error states, keyboard access, visible focus. Add the test that
proves the happy path and the failure path. If the request hides a bigger
feature than one slice, build the slice and say what was deferred.`,
  },
  {
    id: "refactor",
    label: "Refactor",
    hint: "Change the structure, prove the behavior did not move.",
    wantsErrors: false,
    body: (intent) => `## Task

${intent || "Refactor as described above."}

Behavior-preserving: run the project's test suite before touching anything
and again after — same results, or the diff explains why. Complete the
whole change set: callers, tests, docs, examples. No half-migrations, no
compatibility shims left "for later". Delete what the refactor obsoletes.`,
  },
  {
    id: "design",
    label: "Design pass",
    hint: "Make a screen worth using daily, and prove it with measurements.",
    wantsErrors: false,
    body: (intent) => `## Task

${intent || "Run a design pass on the screen described above."}

Load the tanner-design skill before touching any UI. Verify every visual
claim by measurement — screenshot and computed styles, not JSX reading.
Check hierarchy in greyscale, complete the primary flow with keyboard only,
and respect reduced motion. Nothing ships on "looks right in the code".`,
  },
  {
    id: "copy",
    label: "Copy",
    hint: "Words in the product: labels, empty states, docs, store text.",
    wantsErrors: false,
    body: (intent) => `## Task

${intent || "Improve the copy described above."}

Write like Ableton labels: say what the thing does. No "unlock", no
"seamlessly", no "supercharge". Check every claim against the real product
before writing it down — copy that overpromises is a bug. Keep terminology
consistent with the UI's existing words rather than inventing synonyms.`,
  },
  {
    id: "ship",
    label: "Ship check",
    hint: "The honest gate before calling this version done.",
    wantsErrors: true,
    body: (intent) => `## Task

${intent || "Run the pre-ship check on the current state."}

Run every gate the repo has (tests, lint, type check, build) and quote the
real output. Walk the app's primary flows and its loading/empty/error
states. Default verdict is NEEDS WORK — it takes evidence to move off that,
and a suspiciously clean pass is a reason to re-check, not to celebrate.
End with two lists: verified (with how), and not verified (with why).`,
  },
];

export const TEMPLATE_BY_ID: Record<TemplateId, Template> = Object.fromEntries(
  TEMPLATES.map((t) => [t.id, t]),
) as Record<TemplateId, Template>;

/**
 * The footer every generated prompt ends with. The card-update duty is what
 * keeps context cards from rotting; the vault duty is Tanner's standing
 * convention for durable findings.
 */
const FOOTER = `## When you finish

- Update LAUNCHDECK.md in this repo so the next session starts current:
  current focus, known issues, do-not-touch list.
- Write durable findings to the Obsidian vault at
  C:\\Users\\Computer\\Documents\\ClaudeVault (this project's topic folder;
  update the hub's ## Now section).`;

export function assemblePrompt(opts: {
  project: ProjectDto;
  template: Template;
  intent: string;
  card: { exists: boolean; content: string } | undefined;
  errors: ErrorTailDto[];
}): string {
  const { project, template, intent, card, errors } = opts;

  const head = `# ${project.name} — ${template.label.toLowerCase()}

${intent.trim() || "(no intent given — ask before assuming the goal)"}

Repo: ${project.root}${project.runCommand ? `\nRun: ${project.runCommand}` : ""}`;

  const cardSection = card?.exists
    ? `## Context card — LAUNCHDECK.md

${card.content.trim()}`
    : `## Context card — LAUNCHDECK.md

This repo has no context card yet. Create LAUNCHDECK.md at the repo root as
part of this task: stack, key paths, ports, URLs, conventions, current
focus, known issues, and a do-not-touch list.`;

  // A raw-tail fallback is labeled as exactly that. Calling it "Recent
  // errors" would have the prompt inventing errors on every clean log.
  const errorSection =
    template.wantsErrors && errors.length > 0
      ? errors
          .map(
            (t) => `${
              t.matched
                ? `## Recent errors — ${t.source}`
                : `## Last log lines — ${t.source} (no error-marked lines found)`
            }

\`\`\`text
${t.lines.join("\n")}
\`\`\``,
          )
          .join("\n\n")
      : null;

  const hasMatchedErrors = errors.some((t) => t.matched);
  return [head, cardSection, errorSection, template.body(intent, hasMatchedErrors), FOOTER]
    .filter(Boolean)
    .join("\n\n");
}
