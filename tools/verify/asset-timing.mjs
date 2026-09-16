/**
 * How long does it take to *serve* the frontend's own assets?
 *
 * Boot is ~734 ms. About 372 ms of that is WebView2 starting before the page
 * exists and is not ours; ~276 ms is page start to DOM interactive, for one
 * small HTML file, one stylesheet and one module script. That is a lot of
 * milliseconds for three local files, and unlike the WebView2 half it is
 * entirely on our side of the line -- Tauri serves embedded assets through a
 * custom protocol handler written in Rust.
 *
 * Resource Timing knows exactly where each of those milliseconds went, so this
 * asks rather than guesses: per asset, the wait before the first byte and the
 * time spent transferring it.
 *
 * A reload is used rather than the original navigation because the numbers must
 * be attributable per asset, and the first navigation's entries are cluttered by
 * WebView2 initialisation. The protocol handler does the same work either way --
 * it re-serves from the embedded blob, with no HTTP cache in front of it.
 *
 *   CDP_PORT=9280 node tools/verify/asset-timing.mjs
 */
import { connect } from "./cdp.mjs";

const { ws, call, ev } = await connect();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// `--fresh` reads the ORIGINAL navigation instead of reloading.
//
// It matters which one is measured. A reload happens with WebView2 fully
// started, so it isolates the protocol handler; the first navigation happens
// while the engine is still coming up, which is what a user actually waits
// through. Both are worth knowing and they answer different questions --
// running only the reload once made the handler look like the whole story.
const fresh = process.argv.includes("--fresh");
await call("Page.enable");
if (!fresh) await call("Page.reload", { ignoreCache: true });
await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return true;
    await new Promise(r => setTimeout(r, 100));
  }
  return false;
})()`);
await sleep(400);

const report = await ev(`(() => {
  const nav = performance.getEntriesByType("navigation")[0];
  const rows = performance.getEntriesByType("resource")
    .filter((r) => /\\.(js|css|html)$/.test(new URL(r.name).pathname) || r.initiatorType === "script" || r.initiatorType === "link")
    .map((r) => ({
      name: new URL(r.name).pathname.split("/").pop(),
      type: r.initiatorType,
      // Request sent -> first byte back. This is the handler thinking.
      ttfb: +(r.responseStart - r.requestStart).toFixed(1),
      // First byte -> last byte. This is the handler copying.
      transfer: +(r.responseEnd - r.responseStart).toFixed(1),
      total: +(r.responseEnd - r.startTime).toFixed(1),
      bytes: r.decodedBodySize,
      start: +r.startTime.toFixed(1),
    }))
    .sort((a, b) => a.start - b.start);

  return {
    navigation: {
      requestToResponse: +(nav.responseStart - nav.requestStart).toFixed(1),
      responseEnd: +nav.responseEnd.toFixed(1),
      domInteractive: +nav.domInteractive.toFixed(1),
      domContentLoaded: +nav.domContentLoadedEventEnd.toFixed(1),
      loadEvent: +nav.loadEventEnd.toFixed(1),
    },
    rows,
  };
})()`);

const n = report.navigation;
console.log("navigation, relative to the page's time origin:");
console.log(`  document request -> first byte   ${n.requestToResponse.toFixed(1)} ms`);
console.log(`  document fully received          ${n.responseEnd.toFixed(1)} ms`);
console.log(`  DOM interactive                   ${n.domInteractive.toFixed(1)} ms`);
console.log(`  DOMContentLoaded                  ${n.domContentLoaded.toFixed(1)} ms`);
console.log("");
console.log("assets:");
console.log("  name                              start    ttfb  transfer   total     size");
for (const r of report.rows) {
  console.log(
    `  ${r.name.padEnd(32)}${String(r.start).padStart(6)}  ${String(r.ttfb).padStart(6)}  ${String(r.transfer).padStart(8)}  ${String(r.total).padStart(6)}  ${String((r.bytes / 1024).toFixed(0) + " KB").padStart(7)}`,
  );
}

// The question this exists to answer: is the protocol handler the bottleneck,
// or is the browser's own work?
const served = report.rows.reduce((a, r) => a + r.total, 0);
console.log("");
console.log(`total time serving assets: ${served.toFixed(1)} ms of ${n.domContentLoaded.toFixed(1)} ms to DOMContentLoaded`);

ws.close();
