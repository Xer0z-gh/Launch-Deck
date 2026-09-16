/**
 * Attributes the render half of boot to actual functions.
 *
 * After the bundle cut, the largest page-side phase is no longer fetching or
 * parsing JavaScript -- it is the gap between the project list arriving and the
 * rows appearing (~142 ms for 30 rows). That is React render work, and the
 * obvious suspects (a tooltip per action button, an icon per row) are exactly
 * the kind of guess that has been wrong before here.
 *
 * So: start the CPU profiler, reload, and let the whole boot render happen
 * under it. CDP inflates absolute timings, which is why boot totals must never
 * be quoted from a profiled run -- but it inflates the whole page roughly
 * evenly, so the SHARE of time each function takes is still worth reading, and
 * share is what a profile is for.
 *
 *   CDP_PORT=9280 node tools/verify/render-profile.mjs
 */
import { connect } from "./cdp.mjs";

const { ws, call, ev } = await connect();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await call("Profiler.enable");
await call("Profiler.setSamplingInterval", { interval: 100 }); // microseconds
await call("Profiler.start");

await call("Page.enable");
await call("Page.reload", { ignoreCache: false });

// Wait for the rows rather than a fixed sleep, so the profile covers the render
// and stops shortly after instead of burying it in idle samples.
await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return false;
})()`);
await sleep(250);

const { profile } = await call("Profiler.stop");

// Self time per node, then rolled up by function. `nodes` carries hit counts at
// the sampling interval; multiplying by the interval gives milliseconds.
const byId = new Map(profile.nodes.map((n) => [n.id, n]));
const self = new Map();
const intervalMs = 0.1;

for (const node of profile.nodes) {
  const cf = node.callFrame;
  const name = cf.functionName || "(anonymous)";
  const url = (cf.url || "").split("/").pop() || "(native)";
  const key = `${name} @ ${url}`;
  self.set(key, (self.get(key) ?? 0) + (node.hitCount ?? 0) * intervalMs);
}

const total = [...self.values()].reduce((a, b) => a + b, 0);
const ranked = [...self.entries()].sort((a, b) => b[1] - a[1]);

console.log(`profiled ${total.toFixed(0)} ms of CPU across the boot render`);
console.log("(absolute values are CDP-inflated -- read the shares, not the times)");
console.log("");
for (const [key, ms] of ranked.slice(0, 25)) {
  if (ms < 0.5) break;
  console.log(`${ms.toFixed(1).padStart(8)} ms  ${((ms / total) * 100).toFixed(1).padStart(5)}%  ${key}`);
}

// Component-level counts say more than function names once React has minified
// everything into one chunk: 30 rows times N tooltips is a number worth seeing.
const counts = await ev(`JSON.stringify({
  rows: document.querySelectorAll('[data-project-row]').length,
  buttons: document.querySelectorAll('button').length,
  tooltipTriggers: document.querySelectorAll('[data-state][aria-describedby], button[data-state]').length,
  svgs: document.querySelectorAll('svg').length,
  images: document.querySelectorAll('img').length,
  totalNodes: document.querySelectorAll('*').length,
})`);
console.log("");
console.log("DOM at first paint:", counts);

ws.close();
