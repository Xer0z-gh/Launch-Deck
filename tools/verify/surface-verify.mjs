/**
 * Proves the surface pipeline end to end against a server this harness owns.
 *
 * The deterministic path: spin a local HTTP server in-process, wire it as a
 * project's status source THROUGH THE REAL DIALOG (menu → fields → save),
 * assert the chip reports the truth (status + latency), kill the server,
 * re-probe via the chip's own click, assert the chip now says down. Then clear
 * the config through the same dialog and assert the chip is gone.
 *
 * Owning the server is what makes every assertion honest: the harness knows
 * ground truth at each step because it IS the ground truth. Wiring Fleet or
 * VendSuite here instead would couple the suite to whether Tanner's other
 * apps happen to be running.
 *
 *   CDP_PORT=9280 node tools/verify/surface-verify.mjs
 */
import http from "node:http";

import { connect, reporter, showProjects } from "./cdp.mjs";

const { ws, call, ev } = await connect();

// The app opens on the dashboard; this suite measures the project list.
const onProjects = await showProjects(ev);
if (!onProjects) {
  console.error("FAIL: the project list never rendered -- every check below would pass vacuously");
  process.exit(1);
}
const { ok, finish } = reporter();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// The harness's own server: answers 200 with a tiny body.
const server = http.createServer((_, res) => res.end("ok"));
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;

await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);

const click = async (x, y) => {
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
};

/** Real mouse click on the element a selector finds. */
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

// Use the Launch-Deck row itself as the guinea pig; config is cleared at the end.
const TARGET = "Launch-Deck";

// 1. Open the Status source dialog: row menu -> item.
const menuOpened = await clickSel(
  `document.querySelector('[aria-label="More actions for ${TARGET}"]')`,
);
ok("row menu opens", menuOpened);
await ev(`(async () => {
    for (let i = 0; i < 12; i++) {
      if (document.querySelector('[role="menuitem"]')) return true;
      await new Promise(r => setTimeout(r, 250));
    }
    return false;
  })()`);
  // Menu content positions after mount; wait until its rect stops moving.
  await ev(`(async () => {
    const rect = () => JSON.stringify(document.querySelector('[role="menuitem"]')?.getBoundingClientRect() ?? null);
    let prev = rect();
    for (let i = 0; i < 10; i++) {
      await new Promise(r => setTimeout(r, 120));
      const now = rect();
      if (now !== null && now === prev) return true;
      prev = now;
    }
    return false;
  })()`);
const itemClicked = await clickSel(
  `[...document.querySelectorAll('[role="menuitem"]')].find(m => (m.textContent ?? '').includes('Status source'))`,
);
ok("Status source… item exists", itemClicked);
await sleep(700);
ok(
  "dialog opens",
  await ev(`!!document.querySelector('[role="dialog"]')`),
);

// 2. Fill URL + port with the harness server's coordinates and save.
const filled = await ev(`(() => {
  const set = (label, value) => {
    const i = document.querySelector('[role="dialog"] input[aria-label="' + label + '"]');
    if (!i) return false;
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    setter.call(i, value);
    i.dispatchEvent(new Event("input", { bubbles: true }));
    return true;
  };
  return set("URL to ping and open", "http://127.0.0.1:${port}/")
      && set("Local port to probe", "${port}");
})()`);
ok("dialog fields accept input", filled);
await ev(`[...document.querySelectorAll('[role="dialog"] button')].find(b => b.textContent === 'Save')?.click()`);
await sleep(900);
ok("dialog closes on save", await ev(`!document.querySelector('[role="dialog"]')`));

// 3. The chip appears and tells the truth: 200 and a real latency.
let chipText = "";
for (let i = 0; i < 20; i++) {
  chipText = await ev(`(() => {
    const b = document.querySelector('[aria-label^="Re-check status source for ${TARGET}"]');
    return b ? b.textContent.trim() : "";
  })()`);
  if (/200/.test(chipText)) break;
  await sleep(400);
}
ok("chip reports the real 200", /200/.test(chipText), chipText || "no chip");
ok("chip reports latency in ms", /\d+\s*ms/.test(chipText), chipText);

// 4. Select the row. This is where a Radix slot bug once unmounted the whole
// app: the DetailBar renders extra controls for a project WITH a surface URL,
// and a misplaced child inside a Tooltip trigger only crashes on that path --
// so the suite must walk it while the config is live.
const rowAt = await ev(`(() => {
  const tr = [...document.querySelectorAll('[data-project-row]')].find(r => (r.innerText ?? '').includes('${TARGET}'));
  if (!tr) return null;
  tr.scrollIntoView({ block: "center" });
  const b = tr.getBoundingClientRect();
  // Click the STATUS cell, not 300px in: at that offset the point now lands
  // on the row's own surface chip, which stops propagation by design (this
  // very suite asserts that two checks later). The status cell holds only a
  // badge, so it is the one part of a row that is always inert.
  return { x: Math.round(b.left + 40), y: Math.round(b.top + b.height / 2) };
})()`);
ok("the target row is on screen to select", rowAt !== null);
if (rowAt) await click(rowAt.x, rowAt.y);
await sleep(800);
ok(
  "selecting the configured row keeps the app alive",
  await ev(`(document.getElementById('root')?.childElementCount ?? 0) > 0`),
);
ok(
  "detail bar shows the detailed chip and the open button",
  await ev(`!!document.querySelector('footer [aria-label^="Re-check status source for ${TARGET}"]')
         && !!document.querySelector('footer [aria-label="Open ${TARGET} in browser"]')`),
);

// 4b. Enter on the focused chip re-probes WITHOUT opening the log panel --
// the keydown used to bubble to the row, whose Enter handler opens logs.
await ev(`document.querySelector('[aria-label^="Re-check status source for ${TARGET}"]').focus()`);
for (const type of ["keyDown", "keyUp"]) {
  await call("Input.dispatchKeyEvent", {
    type, key: "Enter", code: "Enter",
    windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13,
  });
}
await sleep(600);
ok(
  "Enter on the chip does not drag the log panel open",
  // The aside specifically: every row also has a "Logs for X" BUTTON, which
  // an unqualified selector matches on a healthy screen.
  await ev(`!document.querySelector('aside[aria-label^="Logs for "]')`),
);

// 4c. Narrow bar: the detailed chip yields before the project name. At the
// app's real 644px window the chip once measured the h2 at 0px wide and ran
// 103px under the Run button.
await call("Emulation.setDeviceMetricsOverride", {
  width: 644, height: 461, deviceScaleFactor: 0, mobile: false,
});
await sleep(600);
const narrow = JSON.parse(await ev(`JSON.stringify((() => {
  const h2 = document.querySelector('footer.bar-enter h2');
  const chip = document.querySelector('footer [aria-label^="Re-check status source for ${TARGET}"]');
  return {
    nameWidth: h2 ? Math.round(h2.getBoundingClientRect().width) : -1,
    chipVisible: chip ? chip.offsetParent !== null : false,
  };
})())`));
ok(
  "narrow detail bar keeps the project name readable",
  narrow.nameWidth > 40,
  `h2 width ${narrow.nameWidth}px`,
);
ok(
  "narrow detail bar hides the detailed chip instead of colliding",
  !narrow.chipVisible,
);
await call("Emulation.clearDeviceMetricsOverride", {});
await sleep(500);

// 5. Kill the server, re-probe via the chip's own click, expect honest "down".
await new Promise((r) => server.close(r));
await clickSel(`document.querySelector('[aria-label^="Re-check status source for ${TARGET}"]')`);
let downText = "";
for (let i = 0; i < 20; i++) {
  downText = await ev(`(() => {
    const b = document.querySelector('[aria-label^="Re-check status source for ${TARGET}"]');
    return b ? b.textContent.trim() : "";
  })()`);
  if (/down/.test(downText)) break;
  await sleep(400);
}
ok("chip goes honest-down when the server dies", /down/.test(downText), downText);

// 6. Clear the config through the same dialog; the chip must disappear.
await clickSel(`document.querySelector('[aria-label="More actions for ${TARGET}"]')`);
await ev(`(async () => {
    for (let i = 0; i < 12; i++) {
      if (document.querySelector('[role="menuitem"]')) return true;
      await new Promise(r => setTimeout(r, 250));
    }
    return false;
  })()`);
  // Menu content positions after mount; wait until its rect stops moving.
  await ev(`(async () => {
    const rect = () => JSON.stringify(document.querySelector('[role="menuitem"]')?.getBoundingClientRect() ?? null);
    let prev = rect();
    for (let i = 0; i < 10; i++) {
      await new Promise(r => setTimeout(r, 120));
      const now = rect();
      if (now !== null && now === prev) return true;
      prev = now;
    }
    return false;
  })()`);
await clickSel(
  `[...document.querySelectorAll('[role="menuitem"]')].find(m => (m.textContent ?? '').includes('Status source'))`,
);
await sleep(700);
await ev(`(() => {
  const set = (label) => {
    const i = document.querySelector('[role="dialog"] input[aria-label="' + label + '"]');
    if (!i) return;
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    setter.call(i, "");
    i.dispatchEvent(new Event("input", { bubbles: true }));
  };
  set("URL to ping and open");
  set("Local port to probe");
  set("Log file to watch");
})()`);
await ev(`[...document.querySelectorAll('[role="dialog"] button')].find(b => b.textContent === 'Save')?.click()`);
await sleep(900);
ok(
  "clearing every field removes the chip",
  await ev(`!document.querySelector('[aria-label^="Re-check status source for ${TARGET}"]')`),
);

const code = finish("surface checks");
ws.close();
process.exit(code);
