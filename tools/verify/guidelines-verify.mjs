/**
 * Verifies the web-interface-guideline fixes in the SHIPPED release build,
 * by reading computed styles and live DOM state rather than trusting source.
 *
 * Each check names the failure it prevents, because a green tick that nobody
 * can trace back to a symptom is not evidence of anything.
 */
const PORT = process.env.CDP_PORT ?? "9310";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let page = null;
for (let i = 0; i < 40 && !page; i++) {
  try {
    page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json())
      .find((t) => t.type === "page" && t.webSocketDebuggerUrl);
  } catch { /* not up yet */ }
  if (!page) await sleep(1000);
}
if (!page) { console.error("FAIL: no CDP target"); process.exit(1); }

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r, j) => ((ws.onopen = r), (ws.onerror = j)));
let id = 0;
const call = (method, params) => new Promise((res, rej) => {
  const mid = ++id;
  const timer = setTimeout(() => rej(new Error(`${method} timed out`)), 20000);
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
await call("Runtime.enable");

const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok });
  console.log(`  ${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
};

for (let i = 0; i < 40; i++) {
  if (await ev(`document.querySelectorAll("[data-project-row]").length`)) break;
  await sleep(500);
}

// --- Native chrome follows the app theme -----------------------------------
// Symptom prevented: pale OS scrollbars welded to a charcoal panel.
const chrome = await ev(`(() => ({
  htmlScheme: getComputedStyle(document.documentElement).colorScheme,
  theme: document.documentElement.dataset.theme,
  themeColor: document.querySelector('meta[name="theme-color"]')?.content ?? null,
  bodyOverscroll: getComputedStyle(document.body).overscrollBehaviorY,
}))()`);
check("html color-scheme is set", /dark|light/.test(chrome.htmlScheme), chrome.htmlScheme);
check("color-scheme agrees with data-theme",
  chrome.htmlScheme.includes(chrome.theme ?? "dark"), `${chrome.htmlScheme} vs ${chrome.theme}`);
check("theme-color meta present", chrome.themeColor !== null, chrome.themeColor ?? "missing");
check("body does not rubber-band", chrome.bodyOverscroll === "none", chrome.bodyOverscroll);

// --- Rows: off-screen work skipped, focus visible --------------------------
// Symptom prevented: hundreds of projects all laying out at once.
const rows = await ev(`(() => {
  const row = document.querySelector("[data-project-row]");
  const cs = getComputedStyle(row);
  return { cv: cs.contentVisibility, cis: cs.containIntrinsicSize, height: row.getBoundingClientRect().height };
})()`);
check("rows skip off-screen rendering", rows.cv === "auto", `content-visibility: ${rows.cv}`);
check("rows reserve their height for the scrollbar",
  /60px/.test(rows.cis), `contain-intrinsic-size: ${rows.cis}`);
// 60px is Apple's two-line list row, and the reserved height above must match
// it exactly -- a mismatch makes the scrollbar misreport the list's length.
check("row height matches the iOS list row", Math.round(rows.height) === 60, `${rows.height}px`);

// Focus must be readable ON A SELECTED ROW, where a background change cannot
// work because selection already uses that background.
// `:focus-visible` is heuristic: after a POINTER interaction the browser
// deliberately withholds the ring, so `.click()` then `.focus()` measures the
// mouse path and reports a false failure. Send a real key event first so the
// browser's last-interaction-was-keyboard state matches how a keyboard user
// actually arrives at the row.
for (const type of ["keyDown", "keyUp"]) {
  await call("Input.dispatchKeyEvent", { type, key: "Tab", code: "Tab", windowsVirtualKeyCode: 9 });
}
await sleep(150);

const focus = await ev(`(async () => {
  const row = document.querySelector("[data-project-row]");
  row.click();                       // select it
  row.focus();                       // and focus it
  // React commits selection asynchronously; reading styles in the same tick
  // measures the pre-click render and reports a false negative.
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  const cs = getComputedStyle(row);
  return {
    selected: row.hasAttribute("data-selected"),
    shadow: cs.boxShadow,
    isFocused: document.activeElement === row,
  };
})()`);
check("row is both selected and focused", focus.selected && focus.isFocused,
  `selected=${focus.selected} focused=${focus.isFocused}`);
check("focused row shows an inset ring, not just a background",
  focus.shadow !== "none" && focus.shadow.includes("inset"), focus.shadow);

// --- Text inputs ------------------------------------------------------------
const inputs = await ev(`(() => {
  const els = [...document.querySelectorAll('input[type="text"], input[type="search"], input:not([type])')];
  return els.map((e) => ({
    label: e.getAttribute("aria-label") ?? e.name ?? "?",
    autocomplete: e.autocomplete,
    spellcheck: e.spellcheck,
  }));
})()`);
check("text inputs found", inputs.length > 0, `${inputs.length}`);
check("no text input invites autofill",
  inputs.every((i) => i.autocomplete === "off"),
  inputs.map((i) => `${i.label}=${i.autocomplete}`).join(", "));
check("no text input spellchecks a command or path",
  inputs.every((i) => i.spellcheck === false),
  inputs.map((i) => `${i.label}=${i.spellcheck}`).join(", "));

// --- Typography -------------------------------------------------------------
const typo = await ev(`(() => {
  const h = document.querySelector("h1, h2, h3");
  const metric = document.querySelector("td .tnum");
  return {
    balance: h ? getComputedStyle(h).textWrap || getComputedStyle(h).textWrapStyle : "no heading",
    nowrap: metric ? getComputedStyle(metric).whiteSpace : "no metric cell",
  };
})()`);
check("headings balance their line breaks",
  typo.balance === "balance" || typo.balance === "no heading", typo.balance);
check("number+unit cannot split across lines",
  typo.nowrap === "nowrap" || typo.nowrap === "no metric cell", typo.nowrap);

// --- Dialog scroll containment ---------------------------------------------
// Symptom prevented: a trackpad flick inside a dialog scrolls the page behind.
/**
 * Radix menu triggers listen on `pointerdown`, so element.click() never opens
 * them. Real CDP mouse input is the only faithful way to drive this path.
 */
const centreOf = (expr) => ev(`(() => {
  const el = ${expr};
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };
})()`);

const clickAt = async ({ x, y }) => {
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
};

const dialog = await (async () => {
  const trig = await centreOf(
    `[...document.querySelectorAll("button")].find((b) => (b.textContent ?? "").trim() === "Add")`,
  );
  if (!trig) return { opened: false, why: "no Add trigger" };
  await clickAt(trig);
  await sleep(600);

  const item = await centreOf(
    `[...document.querySelectorAll('[role="menuitem"]')].find((m) => /project folder/i.test(m.textContent ?? ""))`,
  );
  if (!item) return { opened: false, why: "menu did not open" };
  await clickAt(item);
  await sleep(900);

  const res = await ev(`(() => {
    const scroller = [...document.querySelectorAll('[role="dialog"] div')]
      .find((d) => getComputedStyle(d).overflowY === "auto");
    return {
      opened: !!document.querySelector('[role="dialog"]'),
      contain: scroller ? getComputedStyle(scroller).overscrollBehaviorY : "no scroller",
    };
  })()`);

  // Leave the app as we found it.
  await call("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
  await call("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
  await sleep(400);
  return res;
})();
if (dialog.opened) {
  check("dialog body contains its own scrolling", dialog.contain === "contain", dialog.contain);
} else {
  check("dialog opened for inspection", false, dialog.why ?? "unknown");
}

// --- Reduced motion is honoured -------------------------------------------
await call("Emulation.setEmulatedMedia", {
  features: [{ name: "prefers-reduced-motion", value: "reduce" }],
});
await sleep(400);
const motion = await ev(`(() => {
  const row = document.querySelector("[data-project-row]");
  const cs = getComputedStyle(row);
  return { duration: cs.transitionDuration, animation: cs.animationDuration };
})()`);
// 0.01ms rather than 0s is deliberate: a true zero stops `transitionend` from
// firing, which breaks any code awaiting it. Imperceptible is the requirement.
check("reduced motion makes transitions imperceptible",
  parseFloat(motion.duration) <= 0.001, `transition-duration: ${motion.duration}`);
await call("Emulation.setEmulatedMedia", { features: [] });

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} guideline checks passed`);
if (failed.length) console.log("failing:", failed.map((f) => f.name).join("; "));
process.exit(failed.length === 0 ? 0 : 1);
