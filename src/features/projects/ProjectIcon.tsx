import { useQuery } from "@tanstack/react-query";

import { useMonochromeIcon } from "./useMonochromeIcon";
import {
  AppWindow,
  Atom,
  Binary,
  Coffee,
  Cog,
  Container,
  FileCode,
  FileCode2,
  Gem,
  Hexagon,
  Moon,
  Package,
  Smartphone,
  SquareCode,
  Squircle,
  Target,
  Terminal,
  Triangle,
  Zap,
  type LucideIcon,
} from "lucide-react";

import { api, type ProjectDto } from "@/lib/ipc";
import { cn } from "@/lib/cn";

/** Lucide glyphs the bundled runner manifests reference by name. */
const GLYPHS: Record<string, LucideIcon> = {
  "app-window": AppWindow,
  atom: Atom,
  binary: Binary,
  coffee: Coffee,
  cog: Cog,
  container: Container,
  "file-code": FileCode,
  "file-code-2": FileCode2,
  gem: Gem,
  hexagon: Hexagon,
  moon: Moon,
  smartphone: Smartphone,
  "square-code": SquareCode,
  squircle: Squircle,
  target: Target,
  terminal: Terminal,
  triangle: Triangle,
  zap: Zap,
};

/**
 * The project's face: its real icon (Tauri icon set, favicon, or the icon
 * extracted from a built exe) when it has one, else the runner's glyph on a
 * raised tile. Icons never change within a session, so the query never goes
 * stale.
 */
export function ProjectIcon({
  project,
  size,
  className,
}: {
  project: ProjectDto;
  size: number;
  className?: string;
}) {
  // ONE query for the whole library, shared by every row.
  //
  // This used to be keyed per project, which meant 29 IPC round-trips on first
  // paint -- each one a database read plus a filesystem probe -- to answer a
  // question the backend can answer in a single pass. React Query dedupes the
  // identical key, so mounting 29 of these still issues exactly one call.
  //
  // `Infinity` on both: a project's icon does not change while the app runs,
  // and re-probing the filesystem on a refetch would buy nothing.
  const { data: icons } = useQuery({
    queryKey: ["icons"],
    queryFn: () => api.projectIcons(),
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
  });
  const iconUrl = icons?.[project.id] ?? null;
  // White-on-transparency marks vanish on the light theme's white cards.
  // Only those are inverted -- a coloured icon inverted is a negative.
  const monochrome = useMonochromeIcon(iconUrl);

  if (iconUrl) {
    return (
      <img
        src={iconUrl}
        alt=""
        aria-hidden
        width={size}
        height={size}
        draggable={false}
        // No inline style here. There was a `image-rendering: auto` for sizes
        // at or below 24px, which set the property to the value it already
        // had -- `auto` is the CSS initial value and nothing in the app
        // overrides it. Doing nothing is free; doing nothing *via an inline
        // style attribute* is not. Every element carrying one is unique to
        // Blink's computed-style sharing cache, so this opted all thirty row
        // icons out of sharing to express a no-op.
        className={cn(
          "shrink-0 select-none rounded-[3px] object-contain",
          monochrome && "mono-mark",
          className,
        )}
      />
    );
  }

  // No real icon: the runner's glyph on ONE neutral tile, the same for every
  // project. A previous pass tinted this per language, which put twelve hues
  // across the list -- read (correctly) as scattered. Colour now lives in the
  // chrome, where it can tell one story; the rows stay quiet so the eye lands
  // on whatever is actually running.
  const Glyph = GLYPHS[project.glyph ?? ""] ?? Package;
  return (
    <span
      aria-hidden
      className={cn(
        "flex shrink-0 select-none items-center justify-center rounded-[5px]",
        "bg-raised text-ink/45 [box-shadow:var(--hairline-top)]",
        className,
      )}
      style={{ width: size, height: size }}
    >
      <Glyph size={Math.round(size * 0.52)} strokeWidth={1.75} />
    </span>
  );
}
