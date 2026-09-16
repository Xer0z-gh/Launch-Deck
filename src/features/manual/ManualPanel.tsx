/**
 * The manual: what this project is, how to launch it, what else it can do.
 *
 * A persistent right-hand pane that follows the selection, the way a
 * now-playing pane follows the track. Open it once and it stays open while you
 * move down the library, so it answers "what is this one?" without a trip to
 * a detail screen and back.
 *
 * # Everything here is read, never written
 *
 * The summary is the project's own README paragraph. The launch line is the
 * command the runner actually resolved. The extra commands are the scripts
 * really declared in `package.json` or `Cargo.toml`. The docs are files that
 * exist on disk.
 *
 * A generated description would have been easier and would have looked better
 * on every project -- and it would have been a plausible guess that ages badly
 * and cannot be corrected by editing the repo. So a project with no README
 * says it has no README, and the fix is to write one.
 */
import { useQuery } from "@tanstack/react-query";
import { useId } from "react";
import {
  BookOpen,
  ExternalLink,
  FileText,
  FolderOpen,
  Terminal,
  X,
} from "lucide-react";

import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/cn";
import { formatRelative } from "@/lib/format";
import { api, asIpcError, isWebApp, type ProjectDto } from "@/lib/ipc";
import { useUi } from "@/state/ui";

import { ProjectIcon } from "@/features/projects/ProjectIcon";

/** How many docs the pane lists before it says how many more there are. */
const DOC_LIMIT = 8;

export function ManualPanel({ project }: { project: ProjectDto | null }) {
  const toggleManual = useUi((s) => s.toggleManual);

  return (
    <aside
      aria-label="Project manual"
      className={cn(
        "flex w-[340px] shrink-0 flex-col overflow-y-auto border-l border-rule bg-panel",
        // A VIEWPORT query, not a container one: the split this sits in
        // declares no `@container`, so a `@min-[...]` variant here would
        // match nothing and the panel would never appear at all.
        // Below 1100px the pane would leave the library too narrow to read,
        // and the library is the thing you came for.
        "max-[1099px]:hidden",
      )}
    >
      <header className="sticky top-0 z-10 flex h-[52px] shrink-0 items-center gap-2 border-b border-rule bg-panel px-4">
        <BookOpen size={14} aria-hidden className="shrink-0 text-ink/70" />
        <h2 className="min-w-0 flex-1 truncate text-[15px] font-semibold tracking-[-0.24px] text-ink-strong">
          Manual
        </h2>
        <Button
          size="icon"
          onClick={toggleManual}
          aria-label="Close the manual panel"
          className="shrink-0"
        >
          <X size={14} aria-hidden />
        </Button>
      </header>

      {project ? (
        <Body key={project.id} project={project} />
      ) : (
        <p className="px-4 py-6 text-[15px] leading-relaxed text-ink/70">
          Select a project and its manual appears here — what it is, how to
          launch it, and what else it can run.
        </p>
      )}
    </aside>
  );
}

function Body({ project }: { project: ProjectDto }) {
  const toast = useUi((s) => s.toast);
  const openStudio = useUi((s) => s.openStudio);
  const web = isWebApp(project);

  // "Also supports: run only" was a label contradicting its own value.
  const extra = project.supported.filter((l) => l !== "run" && l !== "install");

  const manual = useQuery({
    queryKey: ["manual", project.id],
    queryFn: () => api.projectManual(project.id),
    // Read off disk; only an edit to the repo changes it, and that is rare
    // enough that a session-length cache is right.
    staleTime: 5 * 60_000,
  });

  const desktop = (action: Promise<void>, what: string) => {
    action.catch((e: unknown) => toast("error", `${what}: ${asIpcError(e).message}`));
  };

  return (
    <div className="flex flex-col gap-6 px-4 py-4">
      {/* ---- Identity ------------------------------------------------- */}
      <section aria-labelledby="manual-what">
        <div className="flex items-start gap-3">
          <ProjectIcon project={project} size={38} className="mt-0.5 shrink-0" />
          <div className="min-w-0 flex-1">
            <h3
              id="manual-what"
              className="text-[22px] font-semibold leading-tight tracking-[-0.4px] text-ink-strong"
            >
              {project.name}
            </h3>
            <p className="mt-0.5 text-[13px] text-ink/70">
              {[project.kindLabel, project.version ? `v${project.version}` : null]
                .filter(Boolean)
                .join(" · ")}
            </p>
          </div>
        </div>

        {/* Purpose. The one place the panel could have been tempted to
            invent something, and the one place it must not. */}
        {manual.isLoading ? (
          <div role="status" aria-label="Reading the project's README" className="mt-3 flex flex-col gap-1.5">
            <span className="block h-3 w-full animate-pulse rounded-full bg-fill-4" />
            <span className="block h-3 w-4/5 animate-pulse rounded-full bg-fill-4" />
          </div>
        ) : manual.data?.summary ? (
          <p className="mt-3 text-[15px] leading-relaxed text-ink/85">
            {manual.data.summary}
          </p>
        ) : (
          <p className="mt-3 text-[15px] leading-relaxed text-ink/70">
            {web
              ? "A saved web app. It opens in a window inside Launch Deck."
              : "No README prose in this project, so there is nothing to quote here."}
          </p>
        )}
      </section>

      {/* ---- Actions ----------------------------------------------------
          Directly under the identity, not at the bottom. They used to sit
          1346px into a 772px pane -- 574px below the fold on a project with
          six scripts and six docs. In the now-playing pane this borrows its
          shape from, the transport is pinned, not buried under the credits. */}
      <Group title="Open">
        <div className="flex flex-wrap gap-1.5">
          <Button size="sm" onClick={() => desktop(api.openFolder(project.id), "Open folder")}>
            <FolderOpen size={12} aria-hidden />
            Folder
          </Button>
          {!web && (
            <Button
              size="sm"
              onClick={() => desktop(api.openTerminal(project.id), "Open terminal")}
            >
              <Terminal size={12} aria-hidden />
              Terminal
            </Button>
          )}
          {project.surface?.url && (
            <Button
              size="sm"
              onClick={() => desktop(api.openSurfaceWindow(project.id), "Open app window")}
            >
              <ExternalLink size={12} aria-hidden />
              App window
            </Button>
          )}
          <Button size="sm" onClick={() => openStudio(project.id)}>
            Build prompt…
          </Button>
        </div>
      </Group>

      {/* ---- How to launch it ------------------------------------------ */}
      <Group title="How to launch it">
        {web ? (
          <>
            <Field label="Opens">{project.surface?.url ?? "no URL configured"}</Field>
            <p className="px-0.5 pt-1 text-[13px] leading-relaxed text-ink/70">
              Run opens it in an embedded window. Nothing is spawned, so it
              records no run history.
            </p>
          </>
        ) : (
          <>
            {/* The command and the working directory are NOT repeated here.
                The detail bar 12px below already shows both, and a reference
                pane whose top half duplicates the strip under it is why the
                340px felt unearned on sparse projects. What the bar cannot
                say is what else this project can be told to do. */}
            {extra.length > 0 ? (
              <Field label="Other steps">{extra.join(", ")}</Field>
            ) : (
              <Field label="Other steps">
                Run only — this project declares no dev, build or test step.
              </Field>
            )}
          </>
        )}
        <Field label="Last run">{formatRelative(project.lastLaunchedAt)}</Field>
      </Group>

      {/* ---- What else it can do --------------------------------------- */}
      {(manual.data?.scripts.length ?? 0) > 0 && (
        <Group title="Declared commands">
          <ul className="flex flex-col">
            {manual.data?.scripts.map((s) => (
              <li
                key={s.name}
                className="flex flex-col gap-0.5 border-b border-rule/60 py-2 last:border-b-0"
              >
                <span className="text-[15px] text-ink-strong">{s.name}</span>
                <code className="truncate font-mono text-[12px] text-ink/70" title={s.command}>
                  {s.command}
                </code>
              </li>
            ))}
          </ul>
          <p className="pt-1.5 text-[13px] leading-relaxed text-ink/70">
            Declared in the project's own manifest. Launch Deck runs the
            command above; these are what you would type yourself.
          </p>
        </Group>
      )}

      {/* ---- Where to read more ---------------------------------------- */}
      {(manual.data?.docs.length ?? 0) > 0 && (
        <Group title="Documentation">
          <ul className="flex flex-col">
            {manual.data?.docs.slice(0, DOC_LIMIT).map((d) => (
              <li key={d.path}>
                <button
                  type="button"
                  onClick={() => desktop(api.openPath(d.path), "Open doc")}
                  className={cn(
                    "flex min-h-[32px] w-full items-center gap-2 rounded-control px-1 text-left",
                    "text-[15px] text-ink/85 transition-colors duration-[120ms]",
                    "hover:bg-raised hover:text-ink-strong",
                    "focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent",
                  )}
                >
                  <FileText size={13} aria-hidden className="shrink-0 text-ink/60" />
                  <span className="min-w-0 flex-1 truncate">{d.name}</span>
                </button>
              </li>
            ))}
          </ul>
        </Group>
      )}

    </div>
  );
}

/**
 * A titled block.
 *
 * `aria-labelledby` rather than a bare `<section>`: an unnamed section is a
 * generic element, not a landmark, and `tools/verify/a11y-verify.mjs` asserts
 * that every section in this app is a named region. Four of these were
 * shipping unnamed.
 */
function Group({ title, children }: { title: string; children: React.ReactNode }) {
  const id = useId();
  return (
    <section aria-labelledby={id}>
      <h4
        id={id}
        className="pb-1.5 text-[13px] font-semibold tracking-[-0.078px] text-ink/70"
      >
        {title}
      </h4>
      {children}
    </section>
  );
}

/**
 * A label over its value.
 *
 * Stacked rather than side by side: the values here are paths and commands,
 * which are long, and a two-column layout would truncate every one of them to
 * uselessness in a 340px pane.
 */
function Field({
  label,
  children,
  mono = false,
}: {
  label: string;
  children: React.ReactNode;
  mono?: boolean;
}) {
  return (
    <div className="border-b border-rule/60 py-2 last:border-b-0">
      <div className="text-[13px] text-ink/70">{label}</div>
      <div
        className={cn(
          "mt-0.5 break-words text-[15px] text-ink-strong",
          mono && "font-mono text-[12px] leading-relaxed",
        )}
      >
        {children}
      </div>
    </div>
  );
}
