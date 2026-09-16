/**
 * Drives a real start/stop of one project through the shipped UI.
 *
 * Through the UI rather than `window.__TAURI__`, because release builds do not
 * expose that global -- `ui-verify` asserts its absence as a native-audit
 * check. Real clicks on the real buttons is also the path the user takes, which
 * is the path the bug was on.
 *
 * Driven by `runaway-cli.ps1`, which owns the before/after measurement.
 */
import { connect } from "./cdp.mjs";

const NAME = process.env.DECK_PROJECT ?? "Circle-Calculator";
const { ws, call, ev } = await connect();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const rows = await ev(`(async () => {
  for (let i = 0; i < 60; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return document.querySelectorAll('[data-project-row]').length;
    await new Promise(r => setTimeout(r, 250));
  }
  return 0;
})()`);
if (!rows) {
  console.error("FAIL: no project rows");
  process.exit(1);
}

/** Clicks a button by aria-label with real mouse input, not `.click()`. */
const clickLabel = async (label) => {
  const at = await ev(`(() => {
    const b = document.querySelector(${JSON.stringify(`[aria-label="${label}"]`)});
    if (!b) return null;
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

if (!(await clickLabel(`Run ${NAME}`))) {
  console.error(`FAIL: no Run button for "${NAME}" -- is it registered?`);
  process.exit(1);
}
console.log(`started ${NAME}`);

// Let it get to its menu and settle, so the stop lands on a program that is
// genuinely waiting on stdin -- which is the state that triggered the flood.
await sleep(4000);

const stopped = await clickLabel(`Stop ${NAME}`);
if (!stopped) {
  console.error("FAIL: no Stop button -- the project is not running");
  process.exit(1);
}
const began = Date.now();
console.log("stop requested");

// Wait for it to actually stop. Before the fix this took the full five-second
// grace period; the flood detector should cut it to well under a second.
const gone = await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (!document.querySelector(${JSON.stringify(`[aria-label="Stop ${NAME}"]`)})) return true;
    await new Promise(r => setTimeout(r, 100));
  }
  return false;
})()`);
console.log(`stopped after ${Date.now() - began} ms (clean: ${gone})`);

ws.close();
process.exit(gone ? 0 : 1);
