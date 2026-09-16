/**
 * Proves the four surfaces added for "one app for all my apps" actually work:
 * the grid view, the Storage destination, the manual panel, and the web-app
 * entry point.
 *
 * The assertions here are deliberately about BEHAVIOUR that a screenshot
 * cannot show and a type-check cannot catch: that the grid holds the same
 * projects as the list, that a size bar has real width derived from a real
 * measurement, that expanding a folder fetches its children, and that the
 * manual reports what a project actually says rather than a placeholder.
 *
 *   CDP_PORT=9280 node tools/verify/shell-verify.mjs
 */
import { connect, reporter, showProjects } from "./cdp.mjs";

const { ws, call, ev } = await connect();
const { ok, finish } = reporter();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const click = async (x, y) => {
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
};

/** Clicks the element a selector-expression returns, with a real mouse. */
const clickSel = async (selExpr) => {
  const at = await ev(`(() => {
    const el = ${selExpr};
    if (!el) return null;
    el.scrollIntoView({ block: "center" });
    const r = el.getBoundingClientRect();
    return r.width ? { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) } : null;
  })()`);
  if (!at) return false;
  await click(at.x, at.y);
  await sleep(350);
  return true;
};

/**
 * Waits for a selector to appear, up to ~7s.
 *
 * Deliberately under `cdp.mjs`'s 10s per-call ceiling: a poll loop that runs
 * for the full timeout budget makes the CALL fail rather than the check, and
 * the harness then reports a timeout instead of the assertion that failed.
 */
const waitFor = (sel) =>
  ev(`(async () => {
    for (let i = 0; i < 28; i++) {
      if (document.querySelector(${JSON.stringify(sel)})) return true;
      await new Promise(r => setTimeout(r, 250));
    }
    return false;
  })()`);

const byText = (tag, text) =>
  `[...document.querySelectorAll(${JSON.stringify(tag)})].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

// The app opens on the dashboard; every check below needs the library first.
const onProjects = await showProjects(ev);
ok("the projects list is reachable", onProjects);

// ---------------------------------------------------------------- grid view

const listCount = await ev(`document.querySelectorAll("[data-project-row]").length`);
ok("the list renders rows to compare against", listCount > 0, `${listCount} rows`);

const switchedToGrid = await clickSel(`document.querySelector('button[aria-label="Grid view"]')`);
ok("the grid view switch exists and clicks", switchedToGrid);
await waitFor("[data-project-tile]");

const gridCount = await ev(`document.querySelectorAll("[data-project-tile]").length`);
// The two views are the same data under the same filters. A grid that showed
// a different number would mean one of them is filtering on its own.
ok(
  "the grid holds exactly the projects the list held",
  gridCount === listCount,
  `list ${listCount} vs grid ${gridCount}`,
);

ok(
  "the grid switch reports its state to assistive tech",
  (await ev(`document.querySelector('button[aria-label="Grid view"]')?.getAttribute("aria-pressed")`)) === "true",
);

// Every tile must offer a launch control, per the brief's "every tile has at
// least one action" rule. They are opacity-hidden until hover, so this asks
// the DOM rather than the pixels.
//
// Scoped to the tile's LIST ITEM, not the tile itself: the action used to be
// a descendant of the tile and is now its sibling, because an overlay inside
// the tile swallowed clicks aimed at the tile's centre. The rule being
// asserted -- one launch control per project -- is unchanged; only where it
// lives is, so the selector follows rather than the assertion weakening.
const tilesWithAction = await ev(`
  [...document.querySelectorAll("[data-project-tile]")]
    .filter(t => [...(t.closest("li")?.querySelectorAll("button") ?? [])]
      .some(b => /^(Run|Open|Stop) /.test(b.getAttribute("aria-label") ?? "")))
    .length
`);
ok(
  "every grid tile carries a Run/Open/Stop control",
  tilesWithAction === gridCount,
  `${tilesWithAction} of ${gridCount}`,
);

// Back to the list so later checks and other suites start where they expect.
await clickSel(`document.querySelector('button[aria-label="List view"]')`);
ok("the list view switch returns", await waitFor("[data-project-row]"));

// ----------------------------------------------------------------- storage

const wentToStorage = await clickSel(`${byText("button", "Storage")}`);
ok("Storage opens from the sidebar", wentToStorage);
ok("the Storage screen renders", await waitFor('section[aria-labelledby="storage-projects"]'));

// Sizes stream in. Wait for real measurements rather than asserting on the
// "measuring…" state, which is what the screen shows for its first second.
const measured = await ev(`(async () => {
  for (let i = 0; i < 60; i++) {
    const rows = [...document.querySelectorAll('section[aria-labelledby="storage-projects"] button[aria-expanded]')];
    const done = rows.filter(r => !/measuring/i.test(r.textContent));
    if (done.length >= 3) return done.length;
    await new Promise(r => setTimeout(r, 500));
  }
  return 0;
})()`);
ok("projects report real measured sizes", measured >= 3, `${measured} measured`);

// A size must be a real quantity with a unit, never a bare number.
const sizeText = await ev(`(() => {
  const rows = [...document.querySelectorAll('section[aria-labelledby="storage-projects"] button[aria-expanded]')];
  const row = rows.find(r => /\\d/.test(r.textContent) && !/measuring/i.test(r.textContent));
  return row ? row.textContent.trim() : "";
})()`);
ok(
  "sizes carry their unit",
  /\d+(\.\d+)?\s*(KB|MB|GB)\b/.test(sizeText) || /empty/.test(sizeText),
  sizeText.slice(0, 80),
);

// The bar is the visual encoding of that number, so it must have width that
// came from the measurement -- a decorative full-width bar would be a lie.
// Addressed by `data-size-bar`, not by structure. Two rewrites of this
// selector chased the markup -- `[aria-hidden] > div` broke when the fill
// became a <span>, and `[aria-hidden] > *` then matched the row's chevron
// SVG (also aria-hidden) and read its <path> as the bar. A named hook is the
// same convention `data-project-row` already uses, and it survives layout.
const bars = await ev(`(() => {
  const rows = [...document.querySelectorAll('section[aria-labelledby="storage-projects"] button[aria-expanded="false"]')];
  const widths = rows.map(r => {
    const fill = r.querySelector('[data-size-bar] > *');
    return fill ? parseFloat(fill.style.width) : null;
  }).filter(w => w !== null && !Number.isNaN(w));
  return { count: widths.length, max: Math.max(...widths), min: Math.min(...widths) };
})()`);
ok("size bars are drawn from the measurement", bars.count >= 3, `${bars.count} bars`);
ok(
  "the largest project fills the bar and others do not",
  bars.max >= 99 && bars.min < bars.max,
  `max ${bars.max}% min ${bars.min}%`,
);

// Colour must track absolute size, so a big project and a small one differ.
const distinctBandClasses = await ev(`(() => {
  const rows = [...document.querySelectorAll('section[aria-labelledby="storage-projects"] button[aria-expanded="false"]')];
  const classes = rows.map(r => {
    const fill = r.querySelector('[data-size-bar] > *');
    return fill ? [...fill.classList].find(c => c.startsWith("bg-")) : null;
  }).filter(Boolean);
  return [...new Set(classes)].length;
})()`);
ok(
  "bar colour varies with size rather than being one flat tint",
  distinctBandClasses >= 2,
  `${distinctBandClasses} distinct bands`,
);

// The drill-down: expanding the biggest project must fetch and show children.
const expanded = await clickSel(`
  [...document.querySelectorAll('section[aria-labelledby="storage-projects"] button[aria-expanded]')]
    .find(r => !/measuring/i.test(r.textContent))
`);
ok("a project row expands", expanded);

const childCount = await ev(`(async () => {
  for (let i = 0; i < 60; i++) {
    const kids = document.querySelectorAll('section[aria-labelledby="storage-projects"] ul li');
    const measuring = [...document.querySelectorAll('section[aria-labelledby="storage-projects"] [role="status"]')].length;
    if (kids.length > 0 && measuring === 0) return kids.length;
    await new Promise(r => setTimeout(r, 500));
  }
  return 0;
})()`);
ok("expanding shows the folders inside", childCount > 0, `${childCount} children`);

// Children are ordered largest first -- the whole question this screen answers.
const childOrder = await ev(`(() => {
  const kids = [...document.querySelectorAll('section[aria-labelledby="storage-projects"] ul li')];
  const bars = kids.map(k => {
    const fill = k.querySelector('span[aria-hidden] > span');
    return fill ? parseFloat(fill.style.width) : null;
  }).filter(w => w !== null && !Number.isNaN(w));
  return bars.every((w, i) => i === 0 || bars[i - 1] >= w - 0.01);
})()`);
ok("children are ordered largest first", childOrder);

// Sorting must actually reorder, and must be reversible.
const firstBefore = await ev(`
  document.querySelector('section[aria-labelledby="storage-projects"] button[aria-expanded]')?.textContent.trim().slice(0, 40)
`);
await clickSel(`${byText("button", "Largest first")}`);
await sleep(500);
const firstAfter = await ev(`
  document.querySelector('section[aria-labelledby="storage-projects"] button[aria-expanded]')?.textContent.trim().slice(0, 40)
`);
ok(
  "reversing the sort changes which project is on top",
  firstBefore !== firstAfter,
  `${firstBefore} -> ${firstAfter}`,
);

// ------------------------------------------------------------- manual panel

const manualToggle = `document.querySelector('button[aria-label$="the manual panel"]')`;
const wasOpen = await ev(`${manualToggle}?.getAttribute("aria-pressed") === "true"`);
if (!wasOpen) await clickSel(manualToggle);
ok("the manual panel opens", await waitFor('aside[aria-label="Project manual"]'));

// With nothing selected it must say so rather than render an empty shell.
const emptyText = await ev(`
  document.querySelector('aside[aria-label="Project manual"]')?.textContent ?? ""
`);
ok(
  "the manual states its empty case or shows a project",
  /Select a project|Manual/.test(emptyText),
  emptyText.slice(0, 60),
);

// Select a real project and the panel must fill with ITS facts.
//
// Navigation goes through `showProjects`, not a text match on the sidebar:
// the "All projects" button renders its label AND its count, so its
// textContent is "All projects50" and an equality match silently never fires.
// It did not fire, the suite stayed on Storage, and the three manual checks
// below passed against a project selected earlier -- green, and proving
// nothing. Hence also `expectedName`: the panel must name the row we clicked.
ok("returned to the library to pick a project", await showProjects(ev));
const rowAt = await ev(`(() => {
  const r = document.querySelector("[data-project-row]");
  if (!r) return null;
  const b = r.getBoundingClientRect();
  const name = r.querySelector("span")?.textContent?.trim() ?? "";
  return { x: Math.round(b.left + 40), y: Math.round(b.top + b.height / 2), name };
})()`);
ok("a project row is selectable", rowAt !== null, rowAt?.name ?? "no row");
if (rowAt) await click(rowAt.x, rowAt.y);
await sleep(900);

const manual = await ev(`(() => {
  const panel = document.querySelector('aside[aria-label="Project manual"]');
  if (!panel) return null;
  return {
    heading: panel.querySelector("#manual-what")?.textContent.trim() ?? "",
    text: panel.textContent,
    groups: [...panel.querySelectorAll("h4")].map(h => h.textContent.trim()),
  };
})()`);
// Names THIS project, not merely some project: without comparing against the
// row that was actually clicked, this check passes on a stale selection.
ok(
  "the manual names the project that was just selected",
  Boolean(manual?.heading) && manual?.heading === rowAt?.name,
  `panel "${manual?.heading ?? "none"}" vs row "${rowAt?.name ?? "none"}"`,
);
ok(
  "the manual explains how to launch it",
  (manual?.groups ?? []).includes("How to launch it"),
  (manual?.groups ?? []).join(", "),
);
// The honesty rule: either real README prose, or an explicit statement that
// there is none. Never a fabricated description.
ok(
  "the purpose line is real prose or an explicit absence",
  /No README prose|A saved web app|\w{20,}/.test(manual?.text ?? ""),
);

// -------------------------------------------------------------- web apps

const addOpened = await clickSel(`${byText("button", "Add")}`);
ok("the Add menu opens", addOpened);
await sleep(400);
const hasWebEntry = await ev(`
  [...document.querySelectorAll('[role="menuitem"]')].some(i => /Add a web app/.test(i.textContent))
`);
ok("web apps can be added from the Add menu", hasWebEntry);
await call("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
await call("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });

ws.close();
finish("shell");
