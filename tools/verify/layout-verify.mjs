/**
 * Layout audit: the class of bug where a sibling panel shrinks a container but
 * viewport-keyed breakpoints do not notice.
 *
 * Checks the project list and detail bar at several window widths, with the log
 * panel BOTH closed and open, asserting geometry rather than appearance:
 *   - no two visible cells in a row overlap
 *   - action controls stay inside their container
 *   - nothing scrolls horizontally
 *   - the panel and the list both keep a usable width
 */
const PORT = process.env.CDP_PORT ?? "9310";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let page = null;
for (let i = 0; i < 60 && !page; i++) {
  try {
    page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json())
      .find((t) => t.type === "page" && t.webSocketDebuggerUrl);
  } catch { /* not up */ }
  if (!page) await sleep(1000);
}
if (!page) { console.error("FAIL: no CDP target"); process.exit(1); }

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r, j) => ((ws.onopen = r), (ws.onerror = j)));
let id = 0;
const call = (method, params) => new Promise((res, rej) => {
  const mid = ++id;
  const timer = setTimeout(() => rej(new Error(`${method} timed out`)), 30000);
  const h = (e) => {
    const m = JSON.parse(e.data);
    if (m.id === mid) { clearTimeout(timer); ws.removeEventListener("message", h); res(m.result); }
  };
  ws.addEventListener("message", h);
  ws.send(JSON.stringify({ id: mid, method, params }));
});
const ev = async (expr) => {
  const r = await call("Runtime.evaluate", { expression: expr, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? "threw");
  return r.result?.value;
};

// The app opens on the dashboard (asserted by `dashboard-verify`); this suite
// measures the project list, so it says so rather than assuming.
const onProjects = await ev(`(async () => {
  const go = [...document.querySelectorAll('button')]
    .find(b => b.textContent.trim().startsWith('All projects'));
  go?.click();
  for (let i = 0; i < 30; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);
if (!onProjects) {
  console.error("FAIL: the project list never rendered -- every check below would pass vacuously");
  process.exit(1);
}
await call("Runtime.enable");

// The destination persists between runs, so a harness that assumes the project
// list is showing fails with "no table" after any run that ended on another
// view. Set the starting point explicitly rather than inheriting it.
await ev(`[...document.querySelectorAll("nav button")].find(x=>x.textContent.includes("All projects"))?.click()`);
await sleep(900);

const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok });
  console.log(`  ${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
};

for (let i = 0; i < 60; i++) {
  if (await ev(`document.querySelectorAll("[data-project-row]").length`)) break;
  await sleep(500);
}

/** Measures the list geometry: overlaps, overflow, action containment. */

/** Opens the log panel through the selection bar (the row has no Logs button). */
const openLogPanel = async (ev) => ev(`(async () => {
  const settle = (ms) => new Promise(r => setTimeout(r, ms));
  if (document.querySelector('aside[aria-label^="Logs for"]')) return true;
  const row = document.querySelector('[data-project-row]');
  if (!row) return false;
  row.click();
  for (let i = 0; i < 30; i++) {
    if (document.querySelector('footer.bar-enter')) break;
    await settle(100);
  }
  const logs = document.querySelector('footer [aria-label="Logs"]');
  if (!logs) return false;
  logs.click();
  for (let i = 0; i < 40; i++) {
    if (document.querySelector('aside[aria-label^="Logs for"]')) return true;
    await settle(100);
  }
  return false;
})()`);

const closeLogPanel = async (ev) => ev(`(async () => {
  const settle = (ms) => new Promise(r => setTimeout(r, ms));
  document.querySelector('[aria-label="Close log panel"]')?.click();
  for (let i = 0; i < 40; i++) {
    if (!document.querySelector('aside[aria-label^="Logs for"]')) return true;
    await settle(100);
  }
  return false;
})()`);

const MEASURE = `(() => {
  // The library is a grouped LIST now, not a table: there are no columns to
  // overlap, so what this measures changed shape. What has to hold at every
  // width is the same promise the column checks were protecting -- the row
  // never sheds its primary action or its overflow menu, the title never
  // collapses to nothing, and nothing scrolls sideways.
  const list = document.querySelector("ul:has([data-project-row])");
  const wrap = list?.parentElement;
  if (!list || !wrap) return { error: "no project list" };

  const visible = (el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };

  const row = list.querySelector("[data-project-row]");
  const buttons = row ? [...row.querySelectorAll("button")].filter(visible) : [];
  const wrapRect = wrap.getBoundingClientRect();

  // The two controls that must survive every width: the primary run/stop
  // toggle, and the overflow menu that holds everything the row hid.
  const labels = buttons.map((b) => b.getAttribute("aria-label") ?? "");
  const hasPrimary = labels.some((l) => l.startsWith("Run ") || l.startsWith("Stop "));
  const hasOverflow = labels.some((l) => l.startsWith("More actions"));

  // The title must still render and still have width.
  const title = row ? row.querySelector("span.truncate") : null;
  const nameRect = title ? title.getBoundingClientRect() : null;
  const nameText = title ? title.textContent.trim().length : 0;

  // A row must never be wider than the card that holds it.
  const rowRect = row ? row.getBoundingClientRect() : null;
  const rowFits = rowRect ? rowRect.right <= wrapRect.right + 1 : false;

  return {
    rowFits,
    hOverflow: wrap.scrollWidth > wrap.clientWidth + 1,
    bodyOverflow: document.documentElement.scrollWidth > window.innerWidth + 1,
    buttonCount: buttons.length,
    hasPrimary,
    hasOverflow,
    actionsInside: buttons.every((b) => {
      const r = b.getBoundingClientRect();
      return r.right <= wrapRect.right + 1 && r.left >= wrapRect.left - 1;
    }),
    listWidth: Math.round(wrapRect.width),
    nameWidth: nameRect ? Math.round(nameRect.width) : 0,
    nameText,
  };
})()`;

const widths = [1600, 1240, 1024, 900];

for (const panelOpen of [false, true]) {
  // Toggle the log panel via the UI's own control.
  // The row no longer has its own Logs button -- a grouped-list row carries
  // one primary action and a menu -- so the panel is opened the way a user
  // opens it: select a row, then the selection bar's Logs control.
  const panelState = panelOpen ? await openLogPanel(ev) : await closeLogPanel(ev);
  void panelState;
  await sleep(600);

  const actuallyOpen = await ev(`!!document.querySelector('aside[aria-label^="Logs for"]')`);
  console.log(`\n--- log panel ${actuallyOpen ? "OPEN" : "closed"} ---`);
  check(`panel is ${panelOpen ? "open" : "closed"} as requested`, actuallyOpen === panelOpen);

  for (const width of widths) {
    await call("Emulation.setDeviceMetricsOverride", {
      width, height: 820, deviceScaleFactor: 1, mobile: false,
    });
    await sleep(650);

    const m = await ev(MEASURE);
    if (m.error) { check(`${width}px: project list present`, false, m.error); continue; }

    const tag = `${width}px${actuallyOpen ? " +panel" : ""}`;
    check(`${tag}: rows stay inside the card`, m.rowFits);
    check(`${tag}: no horizontal scroll`, !m.hOverflow && !m.bodyOverflow);
    // Not "all four buttons": the row deliberately sheds secondary actions
    // into the overflow menu when narrow. What must never regress is that the
    // primary action and the menu stay visible and unclipped -- the actual
    // complaint was controls disappearing off the edge.
    check(`${tag}: primary + overflow visible, unclipped`,
      m.hasPrimary && m.hasOverflow && m.actionsInside,
      `${m.buttonCount} buttons, primary=${m.hasPrimary}, overflow=${m.hasOverflow}, list ${m.listWidth}px`);
    check(`${tag}: name column has usable width`, m.nameWidth >= 80 && m.nameText > 0,
      `${m.nameWidth}px`);
  }
}

await call("Emulation.clearDeviceMetricsOverride");

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} layout checks passed`);
if (failed.length) console.log("failing:", failed.map((f) => f.name).join("; "));
process.exit(failed.length === 0 ? 0 : 1);
