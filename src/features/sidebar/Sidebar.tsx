import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Archive,
  CirclePlay,
  HardDrive,
  LayoutDashboard,
  FolderSync,
  Library,
  Star,
  Stethoscope,
} from "lucide-react";
import type { ReactNode } from "react";

import { Button } from "@/components/ui/Button";
import { Tooltip } from "@/components/ui/Tooltip";
import { cn } from "@/lib/cn";
import { api, asIpcError, isLive, type ProjectDto } from "@/lib/ipc";
import { useRuntime } from "@/state/runtime";
import { useUi, type Collection, type Destination } from "@/state/ui";

/**
 * The library rail: fixed collections, then one entry per language present.
 * Counts are honest — they reflect the same filters the main view applies,
 * minus the search query.
 */
/**
 * A top-level destination button.
 *
 * Extracted when the second destination arrived: duplicating the active-state
 * styling is how two controls that should look identical drift apart.
 */
function DestinationItem({
  to,
  label,
  icon,
}: {
  to: Destination;
  label: string;
  icon: React.ReactNode;
}) {
  const destination = useUi((s) => s.destination);
  const setDestination = useUi((s) => s.setDestination);
  const active = destination === to;
  return (
    <button
      type="button"
      onClick={() => setDestination(to)}
      aria-current={active ? "page" : undefined}
      className={cn(
        "relative flex w-full items-center gap-2 rounded-control px-2 py-1.5 text-left text-xs",
        "transition-[background,color,transform] duration-[140ms] ease-[var(--ease-standard)]",
        "hover:translate-x-px",
        active
          ? "bg-raised text-ink before:absolute before:left-0 before:top-1/2 before:h-4 before:w-[2px] before:-translate-y-1/2 before:rounded-full before:bg-accent"
          : "text-ink/60 hover:bg-raised/60 hover:text-ink/90",
      )}
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{label}</span>
    </button>
  );
}

export function Sidebar({ projects }: { projects: ProjectDto[] }) {
  const collection = useUi((s) => s.collection);
  const setCollection = useUi((s) => s.setCollection);
  const states = useRuntime((s) => s.states);

  const active = projects.filter((p) => !p.archived);
  const runningCount = active.filter((p) => isLive(states[p.id])).length;
  const archivedCount = projects.length - active.length;

  const languages = [...new Set(active.map((p) => p.language))].sort((a, b) =>
    a.localeCompare(b),
  );

  const isSelected = (c: Collection) =>
    c.kind === collection.kind &&
    (c.kind !== "language" ||
      (collection.kind === "language" && c.language === collection.language));

  const Item = ({
    c,
    label,
    icon,
    count,
    live,
  }: {
    c: Collection;
    label: string;
    icon?: ReactNode;
    count: number;
    live?: boolean;
  }) => (
    <button
      type="button"
      onClick={() => setCollection(c)}
      aria-current={isSelected(c) ? "true" : undefined}
      className={cn(
        "flex h-9 w-full items-center gap-2.5 rounded-control px-3 text-left",
        "text-[15px] tracking-[-0.24px]",
        "transition-colors duration-[140ms] ease-[var(--ease-standard)]",
        isSelected(c)
          ? // A TINTED fill, not a solid one. iPadOS fills a sidebar
            // selection solidly only while that pane has focus; a permanently
            // solid bar of saturated blue beside a long list is the loudest
            // thing on the screen and drowns everything it is meant to help
            // you find.
            "bg-accent/22 font-medium text-accent"
          : "text-ink/85 hover:bg-fill-4",
      )}
    >
      {icon}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <span
        className={cn(
          "tnum text-[13px]",
          isSelected(c)
            ? "text-accent/75"
            : live && count > 0
              ? "text-accent"
              : "text-ink/45",
        )}
      >
        {count}
      </span>
    </button>
  );

  return (
    <nav
      aria-label="Project collections"
      className="flex w-52 shrink-0 flex-col gap-4 overflow-y-auto border-r border-rule bg-panel px-2.5 py-3"
    >
      <div className="flex flex-col gap-0.5">
        <DestinationItem
          to="dashboard"
          label="Dashboard"
          icon={<LayoutDashboard size={13} className="shrink-0" />}
        />

        <SectionLabel>Library</SectionLabel>
        <Item
          c={{ kind: "all" }}
          label="All projects"
          icon={<Library size={13} className="shrink-0" />}
          count={active.length}
        />
        <Item
          c={{ kind: "favorites" }}
          label="Favourites"
          icon={<Star size={13} className="shrink-0" />}
          count={active.filter((p) => p.favorite).length}
        />
        <Item
          c={{ kind: "running" }}
          label="Running"
          icon={<CirclePlay size={13} className="shrink-0" />}
          count={runningCount}
          live
        />
        <Item
          c={{ kind: "archived" }}
          label="Archived"
          icon={<Archive size={13} className="shrink-0" />}
          count={archivedCount}
        />
      </div>

      <Workspaces />

      {/* Destinations rather than filters, so they render as their own controls
          and does not pretend to be a project collection. Pinned to the bottom
          because it is a tool you reach for when something is wrong, not part
          of the daily path. */}
      <div className="mt-auto flex flex-col gap-0.5 pt-2">
        <SectionLabel>System</SectionLabel>
        <DestinationItem
          to="storage"
          label="Storage"
          icon={<HardDrive size={13} className="shrink-0" />}
        />
        <DestinationItem
          to="diagnostics"
          label="Diagnostics"
          icon={<Stethoscope size={13} className="shrink-0" />}
        />
      </div>

      {languages.length > 0 && (
        <div className="flex flex-col gap-0.5">
          <SectionLabel>Languages</SectionLabel>
          {languages.map((language) => (
            <Item
              key={language}
              c={{ kind: "language", language }}
              label={language}
              count={active.filter((p) => p.language === language).length}
            />
          ))}
        </div>
      )}
    </nav>
  );
}

/**
 * The remembered workspace roots, and the button that rebuilds the library from
 * them.
 *
 * Surfacing these matters: they are the durable seed the whole registry can be
 * re-derived from, so "Rebuild" turns a lost library from a re-do-it-by-hand
 * afternoon into one click. It only ever adds projects, never removes, so it is
 * safe to press at any time -- including to pick up projects added to a
 * workspace since the last scan.
 */
function Workspaces() {
  const queryClient = useQueryClient();
  const toast = useUi((s) => s.toast);
  const openScanDialog = useUi((s) => s.openScanDialog);

  const { data: roots } = useQuery({
    queryKey: ["scanRoots"],
    queryFn: api.scanRoots,
    staleTime: 30_000,
  });

  const rescan = useMutation({
    mutationFn: api.rescanRoots,
    onSuccess: (added) =>
      toast(
        "info",
        added > 0
          ? `Rebuilt from your workspaces: ${added} project(s) added.`
          : "Everything in your workspaces is already registered.",
      ),
    onError: (e: unknown) => toast("error", asIpcError(e).message),
    onSettled: () => queryClient.invalidateQueries({ queryKey: ["projects"] }),
  });

  return (
    <div className="flex flex-col gap-0.5">
      <div className="flex items-center justify-between pr-1">
        <SectionLabel>Workspaces</SectionLabel>
        {(roots?.length ?? 0) > 0 && (
          <Tooltip label="Re-scan remembered workspaces and add anything missing">
            <Button
              size="icon"
              className="h-5 w-5"
              aria-label="Rebuild library from workspaces"
              disabled={rescan.isPending}
              onClick={() => rescan.mutate()}
            >
              <FolderSync size={11} className={rescan.isPending ? "animate-pulse" : ""} />
            </Button>
          </Tooltip>
        )}
      </div>

      {roots === undefined || roots.length === 0 ? (
        <button
          type="button"
          onClick={openScanDialog}
          className="rounded-control px-2 py-1.5 text-left text-[11px] text-ink/40 hover:bg-raised/50 hover:text-ink/70"
        >
          Scan a folder to remember it…
        </button>
      ) : (
        roots.map((root) => (
          <p
            key={root}
            title={root}
            dir="rtl"
            className="truncate px-2 py-1 text-left text-[11px] text-ink/45"
          >
            {root}
          </p>
        ))
      )}
    </div>
  );
}

function SectionLabel({ children }: { children: string }) {
  return (
    <p className="px-3 pb-1 text-[13px] font-semibold tracking-[-0.078px] text-ink/60">
      {children}
    </p>
  );
}
