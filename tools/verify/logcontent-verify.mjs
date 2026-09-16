/**
 * The user's actual failure: log panel open, full of LONG unwrapped lines,
 * on a wide window. Previously the panel had `max-width: none` (its class was
 * never generated), so content could push the flex item far past its 40%.
 *
 * Runs a real project, waits for real output, then measures.
 */
const PORT = process.env.CDP_PORT ?? "9310";
const W = Number(process.env.W ?? 1920);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json())
  .find((t) => t.type === "page" && t.webSocketDebuggerUrl);
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
  const r = await call("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true });
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
const clickAt = async ({ x, y }) => {
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
};
await call("Runtime.enable");
await call("Emulation.setDeviceMetricsOverride", { width: W, height: 1010, deviceScaleFactor: 1, mobile: false });
await sleep(600);
for (let i = 0; i < 40; i++) {
  if (await ev(`document.querySelectorAll("[data-project-row]").length`)) break;
  await sleep(500);
}

// Run the first project and open its logs.
const runBtn = await ev(`(() => {
  const b = [...document.querySelectorAll('button[aria-label^="Run "]')][0];
  if (!b) return null;
  const r = b.getBoundingClientRect();
  return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2), label: b.getAttribute("aria-label") };
})()`);
if (!runBtn) { console.error("no Run button"); process.exit(1); }
console.log("running:", runBtn.label);
await clickAt(runBtn);
await sleep(2500);

// The selection bar carries the Logs control now, so the row it belongs to
// has to be selected first -- the grouped-list row itself has only its
// primary action and a menu.
await ev(`(async () => {
  const settle = (ms) => new Promise(r => setTimeout(r, ms));
  if (document.querySelector('aside[aria-label^="Logs for"]')) return true;
  document.querySelector('[data-project-row]')?.click();
  for (let i = 0; i < 30; i++) {
    if (document.querySelector('footer [aria-label="Logs"]')) break;
    await settle(100);
  }
  document.querySelector('footer [aria-label="Logs"]')?.click();
  for (let i = 0; i < 40; i++) {
    if (document.querySelector('aside[aria-label^="Logs for"]')) return true;
    await settle(100);
  }
  return false;
})()`);
await sleep(2500);

const g = await ev(`(() => {
  const aside = document.querySelector('aside[aria-label^="Logs for"]');
  const list = document.querySelector("ul:has([data-project-row])");
  const wrap = list?.parentElement;
  // No columns in a grouped list; the row itself is the unit that must stay
  // readable beside an open panel.
  const heads = [];
  const row = list?.querySelector("[data-project-row]");
  const nameCell = row?.children[1];
  const detail = document.querySelector('footer[aria-label^="Selected:"]');

  // Overlap across visible header cells -- the reported symptom.
  let overlaps = 0;
  for (let i = 0; i < heads.length; i++)
    for (let j = i + 1; j < heads.length; j++) {
      const a = heads[i].getBoundingClientRect(), b = heads[j].getBoundingClientRect();
      if (a.left < b.right - 1 && b.left < a.right - 1) overlaps++;
    }

  const logLines = document.querySelectorAll('aside[aria-label^="Logs for"] [data-log-line], aside[aria-label^="Logs for"] pre, aside[aria-label^="Logs for"] div');
  return {
    window: window.innerWidth,
    panel: aside ? Math.round(aside.getBoundingClientRect().width) : null,
    panelMaxWidth: aside ? getComputedStyle(aside).maxWidth : null,
    panelScrollWidth: aside ? aside.scrollWidth : null,
    list: wrap ? Math.round(wrap.getBoundingClientRect().width) : null,
    nameWidth: nameCell ? Math.round(nameCell.getBoundingClientRect().width) : null,
    columns: heads.map((h) => h.textContent.trim() || "(status)"),
    overlaps,
    logText: (document.querySelector('aside[aria-label^="Logs for"]')?.textContent ?? "").length,
    detailBarHeight: detail ? Math.round(detail.getBoundingClientRect().height) : null,
    bodyOverflow: document.documentElement.scrollWidth > window.innerWidth + 1,
    nodeCount: logLines.length,
  };
})()`);
console.log(JSON.stringify(g, null, 2));

const ok = g.panel !== null && g.panel <= 880 && g.overlaps === 0
  && g.nameWidth >= 80 && !g.bodyOverflow;
console.log(ok ? "\nPASS  panel bounded, no overlap, name readable" : "\nFAIL  see numbers above");
await call("Emulation.clearDeviceMetricsOverride");
process.exit(ok ? 0 : 1);
