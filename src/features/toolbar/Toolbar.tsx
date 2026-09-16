import {
  BookOpen,
  FolderPlus,
  FileTerminal,
  Globe,
  LayoutGrid,
  List,
  Moon,
  Plus,
  Search,
  Sun,
  SunMoon,
} from "lucide-react";
import { useEffect, useRef } from "react";

import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Menu } from "@/components/ui/Menu";
import { SortMenu } from "./SortMenu";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { useUi, type Theme, type View } from "@/state/ui";

const THEME_ORDER: Theme[] = ["dark", "light", "system"];
const THEME_ICON: Record<Theme, typeof Moon> = {
  dark: Moon,
  light: Sun,
  system: SunMoon,
};

/** App header: identity, search, view toggle, theme, and the add entry point. */
export function Toolbar({ projectCount }: { projectCount: number }) {
  const query = useUi((s) => s.query);
  const setQuery = useUi((s) => s.setQuery);
  const theme = useUi((s) => s.theme);
  const setTheme = useUi((s) => s.setTheme);
  const openAddDialog = useUi((s) => s.openAddDialog);
  const openScanDialog = useUi((s) => s.openScanDialog);
  const openWebDialog = useUi((s) => s.openWebDialog);
  const openShortcutDialog = useUi((s) => s.openShortcutDialog);
  const view = useUi((s) => s.view);
  const setView = useUi((s) => s.setView);
  const destination = useUi((s) => s.destination);
  const manualOpen = useUi((s) => s.manualOpen);
  const toggleManual = useUi((s) => s.toggleManual);
  // The panel has nothing to show where no project can be selected. The
  // WIDTH half of the rule is CSS, not JS -- see the class below.
  const manualUsable = destination === "projects" || destination === "storage";

  const searchRef = useRef<HTMLInputElement>(null);

  // Ctrl+F / Ctrl+K focus the search — the fastest path to any project.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === "f" || e.key === "k")) {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, []);

  const ThemeIcon = THEME_ICON[theme];

  return (
    <header className="flex h-12 shrink-0 items-center gap-3 border-b border-rule bg-panel px-4">
      <div className="flex items-center gap-2">
        {/* The exact geometry of src-tauri/icons/icon.svg, on the same 32-unit
            grid. No tile behind it, matching the taskbar icon -- the mark is
            the identity, and a plate around it was one more box on screen. */}
        <svg viewBox="0 0 32 32" width="18" height="18" aria-hidden>
          <path
            d="M16 4 L29 17 L24.5 21.5 L16 13 L7.5 21.5 L3 17 Z"
            fill="var(--color-ink)"
          />
          <rect x="9" y="24" width="14" height="4" rx="2" fill="var(--color-ink)" />
        </svg>
        <h1 className="text-[13px] font-semibold tracking-tight text-ink">Launch Deck</h1>
        <span className="tnum rounded-full bg-raised px-1.5 py-0.5 text-[10px] text-ink/75">
          {projectCount}
        </span>
      </div>

      <div
        className={cn("relative ml-auto w-72", destination !== "projects" && "invisible")}
        aria-hidden={destination !== "projects"}
      >
        <Search
          size={13}
          aria-hidden
          className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-ink/55"
        />
        <Input
          ref={searchRef}
          // type=search gets the OS's search affordances and Escape-to-clear.
          type="search"
          name="project-search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            // Enter takes the top-ranked hit: focus + select it, so the flow
            // is type -> Enter -> Run without touching the mouse. DOM-driven
            // on purpose -- the first row IS the top hit by construction.
            if (e.key !== "Enter") return;
            // BOTH view shapes. This looked only for `[data-project-row]`, so
            // the documented type-then-Enter flow silently stopped working the
            // day the grid view landed -- Enter left focus in the search box.
            const hit = document.querySelector<HTMLElement>(
              "[data-project-row], [data-project-tile]",
            );
            if (hit) {
              hit.focus();
              hit.click();
            }
          }}
          placeholder="Search projects…  (Ctrl+K)"
          aria-label="Search projects"
          className="pl-8"
        />
      </div>

      <div className="flex items-center gap-1">
        {/* The view switch belongs to the library, so it is absent on the
            screens it cannot change. A control that is visible but inert on
            three of four destinations teaches people to ignore it. */}
        {destination === "projects" && (
          <>
            <ViewSwitch view={view} setView={setView} />
            <SortMenu />
          </>
        )}
        {manualUsable && (
          <Tooltip label={manualOpen ? "Hide the manual" : "Show the manual"}>
            <Button
              size="icon"
              aria-label={manualOpen ? "Hide the manual panel" : "Show the manual panel"}
              aria-pressed={manualOpen}
              onClick={toggleManual}
              className={cn(
                // The SAME rule, in the same mechanism, as the panel's own
                // `max-[1099px]:hidden`. It was a JS `matchMedia` subscription
                // before, and the two could disagree: a resize that CSS saw
                // immediately reached React only when a `change` event fired,
                // leaving a toggle that reported aria-pressed="true" above a
                // pane computing to `display: none`. One rule, one mechanism,
                // nothing to keep in step.
                "max-[1099px]:hidden",
                manualOpen && "bg-accent/15 text-accent",
              )}
            >
              <BookOpen size={14} aria-hidden />
            </Button>
          </Tooltip>
        )}
        <Tooltip label={`Theme: ${theme}`}>
          <Button
            size="icon"
            aria-label={`Theme: ${theme}. Click to change.`}
            onClick={() => {
              const next =
                THEME_ORDER[(THEME_ORDER.indexOf(theme) + 1) % THEME_ORDER.length];
              if (next) setTheme(next);
            }}
          >
            <ThemeIcon size={14} />
          </Button>
        </Tooltip>
      </div>

      <Menu
        align="end"
        // `data-dialog-return` marks where focus goes when a dialog opened
        // from this menu closes: the menu item that opened it is unmounted by
        // then, so Radix would otherwise drop focus on <body>. See `Dialog`.
        trigger={
          <Button variant="primary" size="sm" data-dialog-return>
            <Plus size={13} />
            Add
          </Button>
        }
        items={[
          {
            label: "Add a project folder…",
            icon: <FolderPlus size={13} />,
            onSelect: () => openAddDialog(),
          },
          {
            label: "Scan a workspace…",
            icon: <Search size={13} />,
            onSelect: openScanDialog,
          },
          {
            label: "Add a file, script or folder…",
            icon: <FileTerminal size={13} />,
            section: true,
            onSelect: openShortcutDialog,
          },
          {
            label: "Add a web app…",
            icon: <Globe size={13} />,
            onSelect: openWebDialog,
          },
        ]}
      />
    </header>
  );
}

/**
 * List or grid, as a segmented control.
 *
 * A pair of toggle buttons rather than a menu: there are exactly two options,
 * both are one click away, and the current one is visible without opening
 * anything. `aria-pressed` carries the state, so it is announced rather than
 * conveyed only by the tint.
 */
function ViewSwitch({ view, setView }: { view: View; setView: (v: View) => void }) {
  const options: Array<{ id: View; label: string; Icon: typeof List }> = [
    { id: "list", label: "List", Icon: List },
    { id: "grid", label: "Grid", Icon: LayoutGrid },
  ];
  return (
    <div className="flex items-center gap-0.5 rounded-capsule bg-fill-4 p-0.5">
      {options.map(({ id, label, Icon }) => (
        <Tooltip key={id} label={`${label} view`}>
          <button
            type="button"
            onClick={() => setView(id)}
            aria-label={`${label} view`}
            aria-pressed={view === id}
            className={cn(
              "flex h-[26px] w-[30px] items-center justify-center rounded-capsule",
              "transition-colors duration-[140ms] ease-[var(--ease-standard)]",
              "focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-accent",
              // A tinted fill, not a 1.22:1 background step. Which view is
              // active was carried below 3:1 in both the background and the
              // icon channel, so it read as "nothing is selected".
              view === id
                ? "bg-accent/20 text-accent"
                : "text-ink/70 hover:text-ink-strong",
            )}
          >
            <Icon size={13} aria-hidden />
          </button>
        </Tooltip>
      ))}
    </div>
  );
}
