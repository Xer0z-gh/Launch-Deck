/**
 * Where launch time actually goes, end to end.
 *
 * The backend already breaks its own share into lookup / plan / port_check /
 * spawn, and that share is small. What was never measured is everything
 * AROUND it: the IPC round trip, React's response, and the gap between "we
 * spawned it" and "the OS has a running process". Optimising the instrumented
 * part while the uninstrumented part dominates is the classic way to make a
 * benchmark faster and the product no different.
 *
 * So this times, per project:
 *
 *   click -> the UI shows a live state      (what the user perceives)
 *   click -> a real PID exists              (what actually happened)
 *   the backend's own breakdown             (read from Diagnostics)
 *
 * Every project is launched then stopped, in sequence, so nothing overlaps.
 *
 *   CDP_PORT=9280 node tools/verify/launch-profile.mjs [count]
 */
import { connect } from "./cdp.mjs";

const COUNT = Number(process.argv[2] ?? 6);
const { ws, call, ev } = await connect();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await ev(`(async () => {
  for (let i = 0; i < 60; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);

const clickLabel = async (label) => {
  const at = await ev(`(() => {
    const b = document.querySelector(${JSON.stringify("[aria-label=\"")} + ${JSON.stringify(label)} + '"]');
    if (!b || b.disabled) return null;
    b.scrollIntoView({ block: "center" });
    const r = b.getBoundingClientRect();
    if (r.width === 0) return null;
    return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };
  })()`);
  if (!at) return false;
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", { type, x: at.x, y: at.y, button: "left", clickCount: 1 });
  }
  return true;
};

// Runnable projects, by the name in their Run button.
const names = await ev(`[...document.querySelectorAll('button[aria-label^="Run "]')]
  .filter(b => !b.disabled)
  .map(b => b.getAttribute('aria-label').slice(4))
  .slice(0, ${COUNT})`);

if (!names.length) {
  console.error("FAIL: nothing runnable");
  process.exit(1);
}

console.log(`${names.length} projects\n`);
console.log("project                    perceived   backend   (what the user waits for)");
console.log("--------------------------------------------------------------------------");

const rows = [];
for (const name of names) {
  // Click AND observe inside the page, on one clock.
  //
  // The earlier version armed a watcher over CDP, slept 30ms, then dispatched
  // the click over CDP -- so the arming delay and two round trips sat inside
  // every reading and inflated it by roughly 40ms. That is the same order of
  // magnitude as the thing being measured, and it made an optimistic update
  // that lands in one frame read as 61ms.
  //
  // `.click()` rather than synthetic mouse input is right here: this measures
  // the handler-to-DOM path, and a dispatched event reaches the identical
  // handler. (Hover verification does need real input -- see `prewarm-ab`.)
  // Both numbers from one launch, which is the only way to compare them
  // fairly:
  //
  //   toLive      the optimistic transition -- what the user perceives
  //   toBackend   the row reaching a state only the backend can produce
  //
  // `toBackend` is what the wait WAS before the optimistic update existed, so
  // the pair is a before/after measured on the same click, same machine, same
  // moment. Comparing against a number from an older build would be comparing
  // across machine states, which has already produced one wrong claim here.
  // Selectors built out here, not interpolated into the page script: nesting
  // quotes inside a template literal inside a template literal is how the
  // previous version became a syntax error.
  const runSel = JSON.stringify(`[aria-label="Run ${name}"]`);
  const stopSel = JSON.stringify(`[aria-label="Stop ${name}"]`);

  const wall = Date.now();
  const timings = await ev(`(async () => {
    const run = document.querySelector(${runSel});
    if (!run || run.disabled) return { live: -1, backend: -1 };
    const row = run.closest('tr');
    const started = performance.now();
    run.click();
    let live = -1, backend = -1;
    for (let i = 0; i < 1500; i++) {
      if (live < 0 && document.querySelector(${stopSel})) {
        live = Math.round(performance.now() - started);
      }
      // The badge reads "Running" only for a state the backend produced: the
      // optimistic write is \`starting\`, which renders as "Starting". So this
      // is the wait as it was before the optimistic update existed.
      if (backend < 0 && /Running/.test(row ? row.innerText : "")) {
        backend = Math.round(performance.now() - started);
      }
      if (live >= 0 && backend >= 0) break;
      await new Promise(r => setTimeout(r, 2));
    }
    return { live, backend };
  })()`).catch(() => ({ live: -1, backend: -1 }));
  const toLive = timings.live;
  const toPid = Date.now() - wall;

  rows.push({ name, toLive, toBackend: timings.backend, toPid });
  console.log(
    `${name.padEnd(26)} ${String(toLive).padStart(5)} ms ${String(timings.backend).padStart(7)} ms`,
  );

  await sleep(400);
  await clickLabel(`Stop ${name}`);
  await sleep(700);
}

const med = (xs) => (xs.length ? [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)] : -1);
const live = rows.map((r) => r.toLive).filter((v) => v >= 0);
const back = rows.map((r) => r.toBackend).filter((v) => v >= 0);
console.log("");
console.log(`perceived (optimistic):  median ${med(live)} ms  over ${live.length}`);
console.log(`backend truth arrives:   median ${med(back)} ms  over ${back.length}`);
if (live.length && back.length) {
  console.log(`the wait removed:        ${med(back) - med(live)} ms`);
}

// The backend's own breakdown, for the same launches.
const diag = await ev(`(async () => {
  const nav = [...document.querySelectorAll('nav button, nav a')]
    .find(b => /diagnostic/i.test(b.textContent));
  if (!nav) return "no diagnostics nav";
  nav.click();
  await new Promise(r => setTimeout(r, 1200));
  const text = document.body.innerText;
  const grab = (label) => {
    const m = text.match(new RegExp(label + "[^0-9]*([0-9.]+ ?(?:ms|s|µs))", "i"));
    return m ? m[1] : null;
  };
  return {
    launches: grab("Launches this session"),
    mean: grab("Mean time to spawn"),
    slowest: grab("Slowest"),
  };
})()`);
console.log("");
console.log("backend breakdown:", JSON.stringify(diag));

ws.close();
