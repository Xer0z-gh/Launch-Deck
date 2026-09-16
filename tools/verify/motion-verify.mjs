/**
 * Proves the CSS transitions that replaced motion/react actually animate.
 *
 * Removing an animation library is the kind of change that looks right in a
 * screenshot and is wrong in motion: the panel still ends up open, so every
 * static check passes while the transition has silently become a jump cut. The
 * only honest test samples the animated property over time and asserts it
 * passed through the middle.
 *
 * Each check records the property across the transition and asserts three
 * things: it started at the closed value, it was found at an intermediate
 * value on at least two frames, and it settled at the open value. A jump cut
 * fails the middle assertion while passing the other two, which is exactly the
 * regression this exists to catch.
 *
 * Exit gets the same treatment plus a check that the node is gone afterwards:
 * the presence hook holds it in the DOM deliberately, and a hook that never
 * releases it leaks a node per open.
 *
 *   CDP_PORT=9280 node tools/verify/motion-verify.mjs
 */
import { connect, reporter, showProjects, openLogPanel, closeLogPanel } from "./cdp.mjs";

const { ws, call, ev } = await connect();

// The app opens on the dashboard; this suite measures the project list.
const onProjects = await showProjects(ev);
if (!onProjects) {
  console.error("FAIL: the project list never rendered -- every check below would pass vacuously");
  process.exit(1);
}
const { ok, finish } = reporter();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Pin the media state before measuring: this machine's OS-level "Animation
// effects" toggle feeds prefers-reduced-motion, and with it off the app
// (correctly) collapses every transition -- which this suite would then
// misread as jump-cut regressions. Emulating no-preference measures the
// transitions the app ships; the reduced-motion section below re-emulates
// `reduce` explicitly, so both behaviours stay covered regardless of the OS
// toggle's position.
await call("Emulation.setEmulatedMedia", {
  features: [{ name: "prefers-reduced-motion", value: "no-preference" }],
});
await sleep(150);

// Pin a desktop viewport too: at narrow window widths an open side panel now
// (by design) takes the whole main column, which removes the very elements
// whose transitions this suite measures. 1200px keeps table + panel coexisting
// the way the transitions were designed to be seen.
await call("Emulation.setDeviceMetricsOverride", {
  width: 1200, height: 800, deviceScaleFactor: 0, mobile: false,
});
await sleep(200);

/**
 * Runs `trigger`, then samples `read` every animation frame for `ms`.
 * Sampling on rAF rather than setInterval keeps samples aligned with frames
 * the compositor actually produced, so an intermediate value is one that was
 * really rendered rather than one caught between paints.
 */
const sampleDuring = (trigger, read, ms) => ev(`(async () => {
  ${trigger}
  const samples = [];
  const start = performance.now();
  await new Promise((done) => {
    const tick = () => {
      samples.push(${read});
      if (performance.now() - start < ${ms}) requestAnimationFrame(tick);
      else done();
    };
    requestAnimationFrame(tick);
  });
  return samples;
})()`);

const PANEL_W = `(() => { const el = document.querySelector('aside.panel-enter');
  return el ? +el.getBoundingClientRect().width.toFixed(1) : -1; })()`;
const BAR_H = `(() => { const el = document.querySelector('footer.bar-enter');
  return el ? +el.getBoundingClientRect().height.toFixed(1) : -1; })()`;

/** Shared shape: closed -> intermediate frames -> settled. */
function assertAnimates(label, samples, { from, to, floor }) {
  if (!Array.isArray(samples) || samples.length < 3) {
    ok(`${label} animates`, false, `no samples (${JSON.stringify(samples)})`);
    return;
  }
  const first = samples[0];
  const last = samples[samples.length - 1];
  const lo = Math.min(first, last);
  const hi = Math.max(first, last);
  const mid = samples.filter((v) => v > lo + 2 && v < hi - 2).length;
  // -1 means the node was not in the DOM on the first sampled frame, which is
  // the same fact as "closed" -- it mounts on the click that opens it. Stated
  // separately so a reading of -1 is not reported as though it measured zero.
  if (from !== undefined) {
    ok(
      `${label} starts closed`,
      first === -1 || Math.abs(first - from) <= 2,
      first === -1 ? "not yet mounted" : `${first}px`,
    );
  }
  ok(`${label} animates rather than jumping`, mid >= 2, `${mid} intermediate frames`);
  if (to === "gone") ok(`${label} unmounts after the exit`, last === -1, "node removed");
  else if (floor !== undefined) ok(`${label} settles open`, last >= floor, `${last}px`);
}

const rowsReady = await ev(`(async () => {
  for (let i = 0; i < 60; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return document.querySelectorAll('[data-project-row]').length;
    await new Promise((r) => setTimeout(r, 250));
  }
  return 0;
})()`);
ok("project rows rendered", rowsReady > 0, `${rowsReady} rows`);

// --- detail bar: height 0 -> auto --------------------------------------------
// Selecting a row first, because opening the logs panel needs a row anyway and
// this keeps the two transitions in a deterministic order.
await ev(`document.querySelector('footer.bar-enter [aria-label="Clear selection"]')?.click()`);
await sleep(400);

assertAnimates(
  "detail bar",
  await sampleDuring(`document.querySelector('[data-project-row]').click();`, BAR_H, 340),
  { from: 0, floor: 40 },
);

await sleep(300);
assertAnimates(
  "detail bar exit",
  await sampleDuring(
    `document.querySelector('footer.bar-enter [aria-label="Clear selection"]').click();`,
    BAR_H,
    340,
  ),
  { to: "gone" },
);

// --- log panel: width 0 -> 40% -----------------------------------------------
await sleep(300);
// Select a row first so the selection bar exists; its Logs control is the
// only way in now that the grouped-list row has no per-row Logs button.
await ev(`document.querySelector('[data-project-row]')?.click()`);
await ev(`(async () => {
  for (let i = 0; i < 30; i++) {
    if (document.querySelector('footer [aria-label="Logs"]')) return true;
    await new Promise(r => setTimeout(r, 100));
  }
  return false;
})()`);
assertAnimates(
  "log panel",
  await sampleDuring(
    `document.querySelector('footer [aria-label="Logs"]').click();`,
    PANEL_W,
    420,
  ),
  { from: 0, floor: 300 },
);

await sleep(400);
assertAnimates(
  "log panel exit",
  await sampleDuring(
    `document.querySelector('aside.panel-enter [aria-label="Close log panel"]').click();`,
    PANEL_W,
    420,
  ),
  { to: "gone" },
);

// --- reduced motion ----------------------------------------------------------
// The transition collapsing to ~0ms is the point; what must not change is where
// it ends up. A reduced-motion user still needs the panel open.
await call("Emulation.setEmulatedMedia", {
  features: [{ name: "prefers-reduced-motion", value: "reduce" }],
});
await sleep(200);
// Same route as above: the selection bar's Logs control, because the
// grouped-list row no longer carries one.
await ev(`(async () => {
  document.querySelector('[data-project-row]')?.click();
  for (let i = 0; i < 30; i++) {
    if (document.querySelector('footer [aria-label="Logs"]')) break;
    await new Promise(r => setTimeout(r, 100));
  }
  document.querySelector('footer [aria-label="Logs"]')?.click();
  return true;
})()`);
await sleep(400);
const rmOpen = await ev(PANEL_W);
ok("reduced motion still reaches the open state", rmOpen >= 300, `${rmOpen}px`);

await ev(`document.querySelector('aside.panel-enter [aria-label="Close log panel"]')?.click()`);
await sleep(300);
const rmGone = await ev(PANEL_W);
ok("reduced motion still unmounts", rmGone === -1, "node removed");
await call("Emulation.setEmulatedMedia", { features: [] });

// --- the library is actually gone --------------------------------------------
const residue = await ev(`(async () => {
  const src = document.querySelector('script[type=module]')?.src;
  if (!src) return "no module script";
  const text = await fetch(src).then((r) => r.text());
  // Distinctive motion/react internals. Matching the package name alone would
  // false-positive on a comment that merely mentions it -- and this file's own
  // source does exactly that.
  const hits = ["AnimatePresence", "useMotionValue", "MotionConfigContext", "createMotionComponent"]
    .filter((n) => text.includes(n));
  return hits.length ? "still present: " + hits.join(", ") : "clean";
})()`);
ok("no motion runtime left in the bundle", residue === "clean", residue);

await call("Emulation.clearDeviceMetricsOverride", {});

const code = finish("motion checks");
ws.close();
process.exit(code);
