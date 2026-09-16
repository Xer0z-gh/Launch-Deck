/** View state: filters, panels, theme. Nothing here talks to the backend. */

import { create } from "zustand";

import { api } from "@/lib/ipc";
import type { SortDir, SortKey, SortSpec } from "@/features/projects/sort";
import type { TemplateId } from "@/features/studio/templates";


/**
 * Which top-level destination the main area shows.
 *
 * Deliberately NOT folded into `collection`: a collection is a filter over
 * projects, and diagnostics is not a filter over anything. Overloading one
 * value with both meanings is how "show me diagnostics" ends up filtering the
 * project list to zero rows.
 */
export type Destination = "dashboard" | "projects" | "storage" | "diagnostics";

/**
 * How the library renders.
 *
 * `list` is the grouped list -- dense, sortable, the working view. `grid` is
 * a wall of icons for the "just launch the thing" case, where recognising an
 * app by its mark is faster than reading a column of names. Both show the
 * same projects under the same filters; only the presentation differs, so
 * switching never changes what is on screen.
 */
export type View = "list" | "grid";
export type Theme = "dark" | "light" | "system";

/** Sidebar collections: the LaunchBox-style library groupings. */
export type Collection =
  | { kind: "all" }
  | { kind: "favorites" }
  | { kind: "running" }
  | { kind: "archived" }
  | { kind: "language"; language: string };

export interface Toast {
  id: number;
  kind: "error" | "info";
  message: string;
}

interface UiState {
  query: string;
  collection: Collection;
  /** Selected project (detail bar), if any. */
  selectedId: string | null;
  /** Project whose log panel is open, if any. */
  logsFor: string | null;
  /** Project whose Prompt Studio is open, if any. */
  studioFor: string | null;
  /** Template Prompt Studio opens on ("Debug this" presets debug), if any. */
  studioTemplate: TemplateId | null;
  theme: Theme;
  toasts: Toast[];
  destination: Destination;
  view: View;
  /**
   * Whether the right-hand manual panel is open.
   *
   * A persistent panel rather than a per-project toggle: it follows the
   * selection the way a now-playing pane follows the track, so opening it
   * once keeps it open while you move down the list.
   */
  manualOpen: boolean;
  addDialogPath: string | null;
  webDialogOpen: boolean;
  shortcutDialogOpen: boolean;
  addDialogOpen: boolean;
  scanDialogOpen: boolean;
  /**
   * Explicit column sort, or null for the default ordering (pinned and
   * favourites first, then by name).
   */
  sort: SortSpec | null;

  setQuery: (q: string) => void;
  setCollection: (c: Collection) => void;
  setDestination: (d: Destination) => void;
  setView: (v: View) => void;
  toggleManual: () => void;
  openWebDialog: () => void;
  closeWebDialog: () => void;
  openShortcutDialog: () => void;
  closeShortcutDialog: () => void;
  select: (projectId: string | null) => void;
  openLogs: (projectId: string | null) => void;
  openStudio: (projectId: string | null, template?: TemplateId) => void;
  setTheme: (t: Theme) => void;
  toast: (kind: Toast["kind"], message: string) => void;
  dismissToast: (id: number) => void;
  openAddDialog: (path?: string) => void;
  closeAddDialog: () => void;
  openScanDialog: () => void;
  closeScanDialog: () => void;
  /**
   * Task-Manager behaviour: first click sorts, second click reverses, third
   * clears back to the default order. The third state matters -- without it
   * there is no way back to "pinned first" once you have sorted, short of
   * knowing which column was the default.
   */
  toggleSort: (key: SortKey) => void;
}

const SORT_KEY = "deck.sort.v1";
const VIEW_KEY = "deck.view.v1";
const MANUAL_KEY = "deck.manual.v1";

/** Reads the persisted sort, ignoring anything malformed rather than throwing. */
function storedSort(): SortSpec | null {
  try {
    const raw = localStorage.getItem(SORT_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return null;
    const { key, dir } = parsed as { key?: unknown; dir?: unknown };
    if (typeof key !== "string" || (dir !== "asc" && dir !== "desc")) return null;
    return { key: key as SortKey, dir: dir as SortDir };
  } catch {
    return null;
  }
}

let toastId = 0;

export const useUi = create<UiState>((set) => ({
  query: "",
  // Versioned key: a default-changing redesign bumps it so the new default
  // lands once for existing users, after which their own choice persists again.
  collection: { kind: "all" },
  // The app opens on the LIBRARY.
  //
  // It opened on the dashboard from 2026-08-31, on the argument that "what is
  // the state of everything" is the first question of the morning. That is
  // reversed here, and the counter-argument is Tanner's own (2026-09-14):
  //
  //   "I should be able to open it, immediately find what I need, launch it,
  //    and move on."   ...  "Avoid excessive dashboards"
  //
  // Both statements describe a launcher whose first screen is the thing being
  // launched. The dashboard is one click away in the sidebar and unchanged --
  // this moves it from the doorway to the room next door, because a status
  // board is what you consult, not what you came for.
  destination: "projects",
  view: localStorage.getItem(VIEW_KEY) === "grid" ? "grid" : "list",
  // Closed by default: the panel is a reference surface, and a first run
  // should show the library rather than an empty pane beside it.
  manualOpen: localStorage.getItem(MANUAL_KEY) === "open",
  selectedId: null,
  logsFor: null,
  studioFor: null,
  studioTemplate: null,
  theme: (localStorage.getItem("deck.theme") as Theme | null) ?? "dark",
  toasts: [],
  addDialogPath: null,
  addDialogOpen: false,
  scanDialogOpen: false,
  webDialogOpen: false,
  shortcutDialogOpen: false,
  sort: storedSort(),

  setQuery: (query) => set({ query }),


  // Picking a collection means "show me these projects", so it also leaves
  // diagnostics -- otherwise the click would appear to do nothing.
  setCollection: (collection) =>
    set({ collection, selectedId: null, destination: "projects" }),
  setDestination: (destination) => set({ destination }),

  setView: (view) => {
    localStorage.setItem(VIEW_KEY, view);
    set({ view });
  },

  toggleManual: () =>
    set((s) => {
      const manualOpen = !s.manualOpen;
      localStorage.setItem(MANUAL_KEY, manualOpen ? "open" : "closed");
      return { manualOpen };
    }),

  openWebDialog: () => set({ webDialogOpen: true }),
  closeWebDialog: () => set({ webDialogOpen: false }),
  openShortcutDialog: () => set({ shortcutDialogOpen: true }),
  closeShortcutDialog: () => set({ shortcutDialogOpen: false }),

  select: (selectedId) => set({ selectedId }),

  openLogs: (logsFor) =>
    set(logsFor ? { logsFor, studioFor: null } : { logsFor }),
  openStudio: (studioFor, template) =>
    set(
      studioFor
        ? { studioFor, studioTemplate: template ?? null, logsFor: null }
        : { studioFor, studioTemplate: null },
    ),

  setTheme: (theme) => {
    localStorage.setItem("deck.theme", theme);
    applyTheme(theme);
    set({ theme });
    // Also persist in the database so the preference survives cache clears.
    void api.setSetting("theme", theme).catch(() => undefined);
  },

  toast: (kind, message) =>
    set((s) => {
      toastId += 1;
      const id = toastId;
      setTimeout(() => {
        useUi.getState().dismissToast(id);
      }, 6000);
      return { toasts: [...s.toasts, { id, kind, message }] };
    }),

  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),

  openAddDialog: (path) => set({ addDialogOpen: true, addDialogPath: path ?? null }),
  closeAddDialog: () => set({ addDialogOpen: false, addDialogPath: null }),
  openScanDialog: () => set({ scanDialogOpen: true }),
  closeScanDialog: () => set({ scanDialogOpen: false }),

  toggleSort: (key) =>
    set((s) => {
      const next: SortSpec | null =
        s.sort?.key !== key
          ? { key, dir: "asc" }
          : s.sort.dir === "asc"
            ? { key, dir: "desc" }
            : null;
      if (next) localStorage.setItem(SORT_KEY, JSON.stringify(next));
      else localStorage.removeItem(SORT_KEY);
      return { sort: next };
    }),
}));

const systemDark = window.matchMedia("(prefers-color-scheme: dark)");

/** Stamps the resolved theme onto <html data-theme>. */
export function applyTheme(theme: Theme): void {
  const resolved = theme === "system" ? (systemDark.matches ? "dark" : "light") : theme;
  document.documentElement.dataset["theme"] = resolved;
}

/** Applies the persisted theme and follows OS changes while in system mode. */
export async function initTheme(): Promise<void> {
  const stored = (await api.getSetting("theme").catch(() => null)) as Theme | null;
  const theme = stored ?? useUi.getState().theme;
  useUi.setState({ theme });
  applyTheme(theme);
  systemDark.addEventListener("change", () => {
    if (useUi.getState().theme === "system") applyTheme("system");
  });
}
