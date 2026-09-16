/** Clicks Run on a few projects and reads the backend's own timing breakdown. */
const PORT = process.env.CDP_PORT ?? "9310";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find((t) => t.type === "page" && t.webSocketDebuggerUrl);
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r, j) => ((ws.onopen = r), (ws.onerror = j)));
let id = 0;
const call = (m, p) => new Promise((res, rej) => { const mid = ++id; const t = setTimeout(() => rej(new Error(m)), 30000);
  const h = (e) => { const d = JSON.parse(e.data); if (d.id === mid) { clearTimeout(t); ws.removeEventListener("message", h); res(d.result); } };
  ws.addEventListener("message", h); ws.send(JSON.stringify({ id: mid, method: m, params: p })); });
const ev = async (x) => (await call("Runtime.evaluate", { expression: x, returnByValue: true, awaitPromise: true })).result?.value;
const clickAt = async (c) => { for (const type of ["mousePressed","mouseReleased"]) await call("Input.dispatchMouseEvent", { type, x: c.x, y: c.y, button: "left", clickCount: 1 }); };
await call("Runtime.enable");

// Back to the project list.
const home = await ev(`(() => { const b=[...document.querySelectorAll('nav button')].find(x=>x.textContent.includes('All projects'));
  if(!b) return null; const r=b.getBoundingClientRect(); return {x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2)}; })()`);
if (home) await clickAt(home);
await sleep(900);

// Launch the first few runnable projects, timing the click->state round trip.
for (let i = 0; i < 3; i++) {
  const btn = await ev(`(() => {
    const bs = [...document.querySelectorAll('button[aria-label^="Run "]')].filter(b => !b.disabled);
    const b = bs[${i}];
    if (!b) return null;
    const r = b.getBoundingClientRect();
    return { x: Math.round(r.left + r.width/2), y: Math.round(r.top + r.height/2), label: b.getAttribute("aria-label") };
  })()`);
  if (!btn) break;
  const t0 = Date.now();
  await clickAt(btn);
  // Wait until the row reports a live state.
  let seen = false;
  for (let k = 0; k < 60; k++) {
    seen = await ev(`document.body.textContent.includes("Running") || document.body.textContent.includes("Starting")`);
    if (seen) break;
    await sleep(50);
  }
  console.log(`${btn.label}: UI saw live state after ${Date.now() - t0} ms`);
  await sleep(1200);
}
process.exit(0);
