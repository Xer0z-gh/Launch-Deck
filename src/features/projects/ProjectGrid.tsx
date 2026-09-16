/**
 * The library as a wall of icons.
 *
 * # Why a second view exists at all
 *
 * The grouped list is the working view: it carries a supporting line, a
 * trailing value and an accessory per row, which is what you want when the
 * question is "what is the state of this thing". The grid answers a different
 * question -- "open the one I am picturing" -- where recognising a mark is
 * faster than reading a column of names. Measured against the list in the same
 * viewport: 48 tiles fully visible against 11 rows at 1240x820, 20 against 7
 * at the app's 900x600 minimum. A different density regime, not a restyle.
 *
 * # The structure is what makes it work, and it took two attempts
 *
 * The first version made each tile a `role="button"` div wrapping a full-bleed
 * `bg-panel/85` overlay that held the Run button, because HTML forbids a
 * button inside a button. Two reviews measured what that actually shipped:
 *
 * - The overlay was `opacity: 0` but NOT `pointer-events: none`, so an
 *   invisible Run button owned the centre 24.6% of every tile.
 *   `elementFromPoint` at a hovered tile's centre returned the Run button, and
 *   the reviewer launched two projects by accident while reviewing. The file
 *   claimed "single click selects... the same contract the list row uses";
 *   that contract did not hold at the most natural aim point.
 * - The overlay became opaque exactly when the tile was hovered or focused --
 *   erasing the mark on the one tile you were pointing at, on a screen whose
 *   whole premise is recognising marks.
 * - The focus ring was an INSET box-shadow, which paints on the padding box
 *   BELOW descendants, so the overlay covered it: 1.22:1 against the tile
 *   interior, measured from screenshot pixels. Reading the computed style said
 *   it was fine, which is why one reviewer passed it and the other did not.
 *
 * The fix is structural. The tile is a real `<button>`; the action is a
 * SIBLING in the corner, 26x26 of a 124x100 tile and nowhere near the centre;
 * the ring is a real `outline`, which paints above descendants; nothing ever
 * covers the icon or the label. One activation contract: click, Enter and
 * Space all select, exactly as on a list row, and double-click runs.
 *
 * Roving tabindex, because 50 tiles each holding two tab stops put 100 stops
 * between the search box and the manual panel. The grid is one stop; arrows
 * move within it, in two dimensions, off the live column count.
 */
import { Pin, Play, Square, Star } from "lucide-react";
import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";

import { cn } from "@/lib/cn";
import { canStart, isLive, isWebApp, type ProjectDto } from "@/lib/ipc";
import { useRuntime } from "@/state/runtime";
import { useUi } from "@/state/ui";

import { ProjectIcon } from "./ProjectIcon";
import { useLaunch } from "./useLaunch";
import { useStopProject } from "./useProjects";
import { usePrewarm } from "./usePrewarm";

export function ProjectGrid({ projects }: { projects: ProjectDto[] }) {
  const gridRef = useRef<HTMLUListElement>(null);
  const prewarm = usePrewarm();

  const [active, setActive] = useState(0);
  useEffect(() => {
    // A filter or a search can shrink the list under the cursor.
    setActive((i) => Math.min(i, Math.max(0, projects.length - 1)));
  }, [projects.length]);

  /**
   * How many columns the grid resolved to, right now.
   *
   * The first version offered no Up/Down and said the column count was
   * "decided by the container query rather than by us" -- so ArrowDown fell
   * through to the browser and scrolled the pane 36px while focus stayed put,
   * which is worse than doing nothing. It is one computed-style read.
   */
  const columns = () => {
    const el = gridRef.current;
    if (!el) return 1;
    const tracks = getComputedStyle(el).gridTemplateColumns.split(" ").filter(Boolean);
    return Math.max(1, tracks.length);
  };

  const focusTile = (index: number) => {
    const clamped = Math.max(0, Math.min(projects.length - 1, index));
    setActive(clamped);
    gridRef.current?.querySelectorAll<HTMLElement>("[data-project-tile]")[clamped]?.focus();
  };

  const onKeyDown = (e: ReactKeyboardEvent) => {
    const cols = columns();
    const moves: Record<string, number> = {
      ArrowRight: active + 1,
      ArrowLeft: active - 1,
      ArrowDown: active + cols,
      ArrowUp: active - cols,
      Home: 0,
      End: projects.length - 1,
    };
    const next = moves[e.key];
    if (next === undefined) return;
    e.preventDefault();
    focusTile(next);
  };

  return (
    <div className="@container mx-auto w-full max-w-[1180px] px-1">
      <h2 className="px-1 pb-2 text-[13px] font-semibold tracking-[-0.078px] text-ink/75">
        {projects.length} {projects.length === 1 ? "project" : "projects"}
      </h2>
      <ul
        ref={gridRef}
        onKeyDown={onKeyDown}
        // `auto-fill` rather than a breakpoint ladder. The grid sits beside a
        // sidebar and sometimes a 340px manual panel, so the viewport width
        // says nothing useful about the space it has -- and a container-query
        // ladder still has to guess the steps. The first version used fixed
        // breakpoints and drew three near-empty 210px tiles on a 1240px window.
        className="grid grid-cols-[repeat(auto-fill,minmax(108px,1fr))] gap-1.5"
      >
        {projects.map((project, i) => (
          <Tile
            key={project.id}
            project={project}
            prewarm={prewarm}
            tabbable={i === active}
            onFocus={() => setActive(i)}
          />
        ))}
      </ul>
    </div>
  );
}

function Tile({
  project,
  prewarm,
  tabbable,
  onFocus,
}: {
  project: ProjectDto;
  prewarm: ReturnType<typeof usePrewarm>;
  tabbable: boolean;
  onFocus: () => void;
}) {
  const state = useRuntime((s) => s.states[project.id]);
  const select = useUi((s) => s.select);
  const selected = useUi((s) => s.selectedId === project.id);
  const launch = useLaunch();
  const stop = useStopProject();

  const live = isLive(state);
  const startable = canStart(state) || isWebApp(project);
  const actionable = live || startable;

  return (
    <li className="group relative">
      <button
        type="button"
        data-project-tile
        data-live={live || undefined}
        data-selected={selected || undefined}
        tabIndex={tabbable ? 0 : -1}
        onFocus={onFocus}
        // One contract: click, Enter and Space all select, exactly as they do
        // on a list row. A control announced as a button must not do different
        // things on Enter and Space, and the first version did -- Enter
        // launched, Space selected, with nothing announcing the difference.
        onClick={() => {
          select(project.id);
          prewarm.now(project.id);
        }}
        onDoubleClick={() => launch.run(project)}
        aria-label={`${project.name}, ${project.kindLabel}${live ? ", running" : ""}`}
        aria-pressed={selected}
        className={cn(
          "flex min-h-[104px] w-full cursor-default flex-col items-center justify-center gap-1.5",
          "rounded-card bg-panel px-2 py-2.5",
          "transition-[background-color,transform] duration-[140ms] ease-[var(--ease-standard)]",
          "hover:bg-raised active:scale-[0.99]",
          "data-[selected]:bg-accent/15",
          // A real `outline`, not an inset box-shadow: outline paints above
          // descendants. The inset version measured 1.22:1 the moment
          // anything sat on top of it.
          "focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent",
        )}
      >
        {/* Corner marks. Pin and favourite are the user's own annotations; the
            live dot is the only status this view carries, because a tile
            reporting CPU, memory, version and last-run would be a list row
            drawn as a square. */}
        <span className="absolute left-1.5 top-1.5 flex items-center gap-1">
          {project.pinned && <Pin size={10} aria-hidden className="text-ink/50" />}
          {project.favorite && <Star size={10} aria-hidden className="text-warn" />}
        </span>
        {live && (
          <span
            aria-hidden
            className="absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full bg-accent"
          />
        )}

        {/* 44px, not 32. At 32 the mark was 8.3% of the tile and the tile read
            as a card containing a glyph -- on a screen whose stated premise is
            that recognising a mark beats reading a name. Costs no tiles. */}
        <ProjectIcon project={project} size={44} className="shrink-0" />

        {/* Never covered by anything. Knowing which tile is focused is the
            entire point of focusing it. */}
        <span className="line-clamp-2 px-0.5 text-center text-[12px] leading-tight tracking-[-0.078px] text-ink-strong">
          {project.name}
        </span>
      </button>

      {/* The action, as a SIBLING in the corner rather than an overlay across
          the tile: 26x26 of a 124x100 tile, well clear of the centre.
          `tabIndex={-1}` on purpose and it costs no keyboard access -- the
          grid's double-click, the list row, the row menu and the manual panel
          all run a project. Making it tabbable would put a second stop on
          every tile, which is half of the 100 stops that were measured. */}
      {actionable && (
        <button
          type="button"
          tabIndex={-1}
          onClick={() => {
            if (live) stop.mutate({ id: project.id });
            else launch.run(project);
          }}
          aria-label={live ? `Stop ${project.name}` : `Run ${project.name}`}
          className={cn(
            "absolute bottom-1.5 right-1.5 flex h-[26px] w-[26px] items-center justify-center",
            // An opaque backing, so the glyph is measured against one known
            // surface instead of against whatever icon happens to be behind
            // it. The old capsule stacked accent on accent/15 on panel/85 and
            // measured 3.99-4.10:1 for its label.
            "rounded-full bg-raised",
            "opacity-0 transition-opacity duration-[140ms] ease-[var(--ease-standard)]",
            "group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100",
            "hover:bg-fill-3",
            "focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent",
            live ? "text-signal" : "text-accent",
          )}
        >
          {live ? <Square size={11} aria-hidden /> : <Play size={11} aria-hidden />}
        </button>
      )}
    </li>
  );
}
