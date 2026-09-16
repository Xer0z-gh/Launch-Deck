/**
 * Guards the most expensive thing a list can do to Chromium's style engine:
 * give every row its own inline style.
 *
 * # What this is about
 *
 * Blink shares one computed style between elements that match the same rules.
 * An element carrying an inline style attribute is unique by definition and
 * cannot participate, so it computes from scratch and drags its subtree with
 * it. Thirty rows with `--row-i: 0..29` meant thirty full computations of the
 * largest repeated structure on the screen, and the sharing cache went unused
 * exactly where it would have paid most.
 *
 * Style recalc was **167 ms of boot against 11 ms of layout**, so this was the
 * dominant cost of showing a list -- not React, not rendering, not the bundle.
 *
 * # Two assertions and a measurement
 *
 * The assertions are the guard: rows must carry no inline style, and neither
 * must their repeated descendants. Those are cheap to check and they are what
 * regresses -- `style={{ "--row-i": index }}` is a natural thing to write.
 *
 * The measurement is the justification, kept runnable so the claim can be
 * re-checked rather than believed. It reintroduces the inline property, times
 * both conditions, and reports the difference.
 *
 * # Why the measurement is shaped the way it is
 *
 * Two earlier versions of this probe were wrong in the same way, and the shape
 * below is the fix for both:
 *
 *   - **Both conditions in ONE page, interleaved.** Comparing across reloads
 *     produced a confident 54% "finding" from a property the build under test
 *     no longer had.
 *   - **After every animation has finished.** A running CSS animation forces a
 *     recalc every frame and roughly doubles a forced synchronous recalc, so a
 *     probe that starts too early measures animation state instead of the
 *     change. The row entrance cascade runs for up to 450 ms after paint.
 *
 *   CDP_PORT=9280 node tools/verify/style-cost.mjs
 */
import { connect, reporter } from "./cdp.mjs";

const { ws, call, ev } = await connect();
const { ok, finish } = reporter();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Reload first. The guard below asserts that no row carries an inline style,
// and the measurement further down deliberately adds one to thirty rows -- so a
// second run against a page this script has already touched would report its
// own leftovers as a product regression. It did exactly that once.
await call("Page.enable");
await call("Page.reload", {});

const rows = await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return document.querySelectorAll('[data-project-row]').length;
    await new Promise(r => setTimeout(r, 100));
  }
  return 0;
})()`);
ok("project rows rendered", rows > 0, `${rows} rows`);

// --- the guard ---------------------------------------------------------------
const styledRows = await ev(`document.querySelectorAll('[data-project-row][style]').length`);
ok(
  "no inline style on any row",
  styledRows === 0,
  styledRows === 0
    ? "rows share one computed style"
    : `${styledRows} rows carry one — use :nth-child in tokens.css, not an inline custom property`,
);

const styledDescendants = await ev(`JSON.stringify(
  [...document.querySelectorAll('[data-project-row] [style]')]
    .reduce((acc, el) => {
      const k = el.tagName.toLowerCase() + ' [' + el.getAttribute('style') + ']';
      acc[k] = (acc[k] ?? 0) + 1;
      return acc;
    }, {})
)`);
const groups = JSON.parse(styledDescendants);
const repeated = Object.entries(groups).filter(([, n]) => n >= 5);
// A handful is noise; the same inline style repeated once per row is the
// pattern that defeats sharing, and it is the count that makes it expensive.
ok(
  "no inline style repeated once per row",
  repeated.length === 0,
  repeated.length === 0
    ? "none"
    : repeated.map(([k, n]) => `${n}× ${k}`).join("; "),
);

// --- the measurement ---------------------------------------------------------
await sleep(800);
const settled = await ev(`(async () => {
  for (let i = 0; i < 60; i++) {
    if (document.getAnimations().filter(a => a.playState === 'running').length === 0) return true;
    await new Promise(r => setTimeout(r, 100));
  }
  return false;
})()`);
ok("animations settled before timing", settled === true, settled ? "quiet" : "still running — timing would be noise");

const out = await ev(`(() => {
  const rows = [...document.querySelectorAll('[data-project-row]')];
  const time = () => {
    const r = [];
    for (let k = 0; k < 3; k++) {
      document.documentElement.style.setProperty('--probe', String(Math.random()));
      const t0 = performance.now();
      void document.body.offsetHeight;
      r.push(performance.now() - t0);
    }
    return r.sort((a, b) => a - b)[1];
  };
  const withProp = () => rows.forEach((el, i) => el.style.setProperty('--row-i', String(i)));
  const without = () => rows.forEach((el) => el.removeAttribute('style'));

  const A = [], B = [];
  without(); time(); withProp(); time(); without(); time();
  for (let i = 0; i < 14; i++) {
    // Alternate the order so a systematic first-after-mutation cost cannot
    // land on the same condition every time.
    if (i % 2 === 0) { without(); A.push(time()); withProp(); B.push(time()); }
    else { withProp(); B.push(time()); without(); A.push(time()); }
  }
  without();
  const med = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
  return JSON.stringify({ medWithout: med(A), medWith: med(B), maxWithout: Math.max(...A), minWith: Math.min(...B) });
})()`);

const m = JSON.parse(out);
const delta = m.medWith - m.medWithout;
console.log("");
console.log(`  forced full-document recalc, shipped:          ${m.medWithout.toFixed(1)} ms`);
console.log(`  the same page with one inline prop per row:    ${m.medWith.toFixed(1)} ms`);
console.log(`  cost of the inline property:                   ${delta.toFixed(1)} ms (${((delta / m.medWith) * 100).toFixed(0)}%)`);
console.log("");

// Separation, not just a difference in medians: if the slowest shipped sample
// still beats the fastest instrumented one, the two populations do not overlap
// and the effect is not a statistical artifact of a busy machine.
ok(
  "the two conditions do not overlap",
  m.maxWithout < m.minWith,
  `slowest shipped ${m.maxWithout.toFixed(1)} ms < fastest instrumented ${m.minWith.toFixed(1)} ms`,
);

const code = finish("style-cost checks");
ws.close();
process.exit(code);
