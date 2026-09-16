/**
 * Proves the dashboard reports the truth and acts, rather than decorating.
 *
 * The assertions that matter here are CONSISTENCY assertions: the dashboard
 * derives its numbers from the same live queries as the rest of the app, so
 * every headline must agree with the surface it summarises. A dashboard whose
 * tiles drift from the list beneath them is worse than no dashboard, because
 * it is believed.
 *
 * Ground truth comes from the app's own DOM in the same pass (the projects
 * table, the Today strip), never from a second source that could itself be
 * wrong.
 *
 *   CDP_PORT=9280 node tools/verify/dashboard-verify.mjs
 */
import { connect, reporter } from "./cdp.mjs";

const { ws, call, ev } = await connect();
const { ok, finish } = reporter();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const click = async (x, y) => {
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
};
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
  return true;
};
const goDashboard = async () => {
  await clickSel(`[...document.querySelectorAll('button')].find(b => b.textContent.trim() === 'Dashboard')`);
  await ev(`(async () => {
    for (let i = 0; i < 40; i++) {
      if (document.querySelector('section[aria-labelledby="dash-now"]')) return true;
      await new Promise(r => setTimeout(r, 250));
    }
    return false;
  })()`);
  await sleep(600);
};

// 1. The app LANDS on the LIBRARY, and the dashboard is one click away.
//
// ASSERTION INVERTED, 2026-09-14. This used to read "the app opens on the
// dashboard". Tanner's brief that day asked for the opposite in his own words
// -- "open it, immediately find what I need, launch it, and move on", and
// "avoid excessive dashboards" -- so the product decision changed and the test
// encodes the new rule rather than being deleted. The dashboard still has to
// be reachable and correct, which is what the rest of this suite checks.
await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (document.querySelector('[data-project-row], [data-project-tile]')) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);
ok(
  "the app opens on the library, not the dashboard",
  await ev(`!!document.querySelector('[data-project-row], [data-project-tile]')
            && !document.querySelector('section[aria-labelledby="dash-now"]')`),
);
ok("the dashboard is one click from there", await (async () => {
  await clickSel(`[...document.querySelectorAll('button')].find(b => b.textContent.trim() === 'Dashboard')`);
  return ev(`(async () => {
    for (let i = 0; i < 40; i++) {
      if (document.querySelector('section[aria-labelledby="dash-now"]')) return true;
      await new Promise(r => setTimeout(r, 250));
    }
    return false;
  })()`);
})());

// BEFORE settling: the header must not assert a verdict about surfaces it
// has not measured yet. The contradiction this catches only exists in the
// ~1.3s window while probes resolve -- and `run-all` waits 6s after launch,
// so by the time this suite starts, that window is long gone. RELOAD to
// recreate it, then sample immediately; without this the check passes on
// "0 rows checking" every run and guards nothing.
await call("Page.enable");
await call("Page.reload", {});
// A reload lands on the LIBRARY now, not the dashboard -- the app's opening
// screen changed on 2026-09-14. So the navigation has to happen again, and it
// has to happen FAST, because the window this check samples is the ~1.3s while
// surface probes are still resolving. Waiting for rows first and then clicking
// through would spend that window and make the check vacuous, which is exactly
// the failure mode the comment above describes.
await ev(`(async () => {
  for (let i = 0; i < 60; i++) {
    const nav = [...document.querySelectorAll('button')]
      .find(b => b.textContent.trim() === 'Dashboard');
    if (nav) { nav.click(); return true; }
    await new Promise(r => setTimeout(r, 25));
  }
  return false;
})()`);
await ev(`(async () => {
  for (let i = 0; i < 40; i++) {
    if (document.querySelector('section[aria-labelledby="dash-now"]')) return true;
    await new Promise(r => setTimeout(r, 50));
  }
  return false;
})()`);
const early = JSON.parse(await ev(`JSON.stringify((() => {
  const header = document.querySelector('main h2')?.parentElement?.textContent ?? "";
  const checking = [...document.querySelectorAll('section[aria-labelledby="dash-watched"] *')]
    .filter(el => el.children.length === 0 && /checking/i.test(el.textContent ?? "")).length;
  return { header: header.replace(/\\s+/g, ' '), checking };
})())`));
ok(
  "the loading window was actually observed",
  early.checking > 0,
  `${early.checking} rows still checking right after reload`,
);
ok(
  "while probes are unresolved the header says so instead of claiming health",
  early.checking === 0 || /checking/i.test(early.header),
  `${early.checking} rows checking; header: ${early.header.slice(0, 90)}`,
);

// Let the surface probes settle so the tiles hold readings, not "checking".
await sleep(3500);

// 2. The headline tiles keep their contract: four of them, each naming
// itself from its own content, and NONE of them a button that does nothing.
//
// A `StatTile` deliberately renders a plain div when it has no action --
// an audit measured the zero-value "Died" tile as a focusable button that
// swallowed Enter. And no tile carries an `aria-label`, because a label on
// a control REPLACES its content: the visible word "Library" vanished from
// the accessible name, and every detail line with it.
const tiles = JSON.parse(await ev(`JSON.stringify((() => {
  const section = document.querySelector('section[aria-labelledby="dash-now"]');
  const grid = [...(section?.children ?? [])].find(el => el.className.includes('grid-cols-2'));
  return [...(grid?.children ?? [])].map(el => ({
    tag: el.tagName,
    ariaLabel: el.getAttribute('aria-label') ?? "",
    visibleLabel: el.querySelector('span')?.textContent?.trim() ?? "",
    text: el.textContent.replace(/\\s+/g, ' ').trim(),
    srOnly: [...el.querySelectorAll('.sr-only')].map(n => n.textContent.trim()).join(" "),
    describedBy: el.getAttribute('aria-describedby') ?? "",
  }));
})())`));
ok("four headline tiles", tiles.length === 4, `${tiles.length} tiles`);
ok(
  "no tile overrides its own content with aria-label",
  tiles.every((t) => t.ariaLabel === ""),
  tiles.map((t) => `${t.visibleLabel}:${t.ariaLabel || "-"}`).join(" | "),
);
ok(
  "every tile's name starts with its visible label (Label-in-Name)",
  tiles.every((t) => t.visibleLabel.length > 0 && t.text.startsWith(t.visibleLabel)),
  tiles.map((t) => `${t.visibleLabel}|${t.text.slice(0, 18)}`).join(" ; "),
);
ok(
  "a tile is a button only when it actually acts",
  tiles.every((t) =>
    t.tag === "BUTTON" ? t.srOnly.length > 0 : t.tag === "DIV" && t.srOnly.length === 0,
  ),
  tiles.map((t) => `${t.tag}:${t.srOnly || "no-action"}`).join(" | "),
);
ok(
  "tiles expose their detail line as a description, not by swallowing it",
  tiles.filter((t) => t.tag === "BUTTON").every((t) => t.describedBy.length > 0),
  tiles.map((t) => `${t.visibleLabel}:${t.describedBy || "-"}`).join(" | "),
);

// 3. Consistency: the Running tile must agree with the projects table and
// with the Today strip. Three surfaces, one truth. `\\s*` because
// textContent concatenates the label and value with no separator.
const runningTile = tiles.find((t) => /^Running/i.test(t.visibleLabel));
const tileCount = Number(/^Running\s*(\d+)/i.exec(runningTile?.text ?? "")?.[1] ?? "-1");
await clickSel(`[...document.querySelectorAll('button')].find(b => b.textContent.trim().startsWith('All projects'))`);
await sleep(900);
const liveRows = await ev(
  `document.querySelectorAll('[data-project-row][data-live]').length`,
);
const stripText = await ev(`document.querySelector('[aria-label="Today"]')?.textContent ?? ""`);
const stripCount = /(\d+) running/.test(stripText)
  ? Number(/(\d+) running/.exec(stripText)[1])
  : 0;
ok(
  "dashboard, table and Today strip agree on what is running",
  tileCount === liveRows && stripCount === liveRows,
  `tile ${tileCount}, table ${liveRows}, strip ${stripCount}`,
);

// 4. The watched card lists exactly the wired projects -- the same ones that
// carry a chip in the table.
const chipCount = await ev(
  `document.querySelectorAll('[data-project-row] [aria-label^="Re-check status source for "]').length`,
);
await goDashboard();
const watchedRows = await ev(
  `document.querySelectorAll('section[aria-labelledby="dash-watched"] [class*="border-b"]').length`,
);
ok(
  "the watched card covers every wired project",
  watchedRows === chipCount,
  `dashboard ${watchedRows}, table chips ${chipCount}`,
);

// 5. Readings render as measurements, not placeholders: at least one real
// HTTP reading (`200 · N ms`) or port/log age, and no bare zeros.
const watchedText = await ev(
  `document.querySelector('section[aria-labelledby="dash-watched"]')?.textContent ?? ""`,
);
ok(
  "watched readings are real measurements",
  /\d{3}\s·\s\d+\sms/.test(watchedText) || /log \d+[smhd]/.test(watchedText),
  watchedText.replace(/\s+/g, " ").slice(0, 80),
);

// 6. No run is reported as "running" while the dashboard also says nothing
// is running -- the stale-history contradiction this suite exists to pin.
const activityText = await ev(
  `document.querySelector('section[aria-labelledby="dash-activity"]')?.textContent ?? ""`,
);
const claimsRunningRows = (activityText.match(/running/g) ?? []).length;
ok(
  "activity rows never contradict the running count",
  liveRows > 0 || claimsRunningRows === 0,
  `table live ${liveRows}, activity rows saying running ${claimsRunningRows}`,
);

// 6b. Header and tiles, read in ONE evaluate so they cannot be sampled from
// different frames, and asserted in BOTH directions: a zero-down tile must
// not sit under a header announcing outages, and a down tile must be named
// with the same denominator.
const frame = JSON.parse(await ev(`JSON.stringify((() => {
  const section = document.querySelector('section[aria-labelledby="dash-now"]');
  const grid = [...(section?.children ?? [])].find(el => el.className.includes('grid-cols-2'));
  const tile = [...(grid?.children ?? [])].find(el => /^Surfaces? down/i.test(el.textContent ?? ""));
  return {
    header: (document.querySelector('main h2')?.parentElement?.textContent ?? "").replace(/\\s+/g, ' '),
    tileText: (tile?.textContent ?? "").replace(/\\s+/g, ' '),
  };
})())`));
const frameDown = Number(/down\s*(\d+)/i.exec(frame.tileText)?.[1] ?? "-1");
ok(
  "the header sentence agrees with the surfaces-down tile, in one frame",
  frameDown < 0
    ? false
    : frameDown > 0
      // The exact count AND the same denominator, word-bounded so "41 of 7"
      // cannot satisfy a check about 1.
      ? new RegExp(`\\b${frameDown} of (\\d+) watched`).test(frame.header)
      // Zero down: the header must not be announcing outages.
      : !/\d+ of \d+ watched/.test(frame.header),
  `tile says ${frameDown} down; header: ${frame.header.slice(0, 110)}`,
);

// 7. A tile acts: clicking Library lands on the projects list.
await clickSel(`[...document.querySelectorAll('section[aria-labelledby="dash-now"] button')].find(b => /LIBRARY/i.test(b.textContent))`);
await sleep(900);
ok(
  "the Library tile navigates to the projects list",
  await ev(`document.querySelectorAll('[data-project-row]').length > 0`),
);

// 8. Heading outline: exactly one h1 in the document, and the dashboard's
// own title sits under it rather than competing with it.
await goDashboard();
const headings = JSON.parse(await ev(`JSON.stringify(
  [...document.querySelectorAll('h1, h2, h3')].map(h => h.tagName + ':' + h.textContent.trim().slice(0, 18))
)`));
ok(
  "one h1, dashboard title below it",
  headings.filter((h) => h.startsWith("H1")).length === 1 &&
    headings.some((h) => h === "H2:Dashboard"),
  headings.join(" | "),
);

const code = finish("dashboard checks");
ws.close();
process.exit(code);
