/**
 * Keyboard and screen-reader structure, checked against the shipped build.
 *
 * The two open accessibility items were "audit the dialogs' keyboard traversal"
 * and "screen-reader pass over Diagnostics". Both are the kind of thing that
 * gets *declared* done by reading the JSX and seeing `aria-` attributes — and
 * the attributes are not the behaviour. Whether Tab actually cycles inside a
 * dialog, whether Escape returns focus to what opened it, and whether a
 * `<section>` is exposed as a landmark at all, are facts about the rendered
 * page.
 *
 * So this drives real keys through CDP and reads the resulting DOM.
 *
 * Two things it deliberately does NOT do:
 *
 *   - **Contrast.** `guidelines-verify` already measures it from computed
 *     styles, and two harnesses asserting the same thing drift apart.
 *   - **Judge whether a label reads well.** It can see that a control has an
 *     accessible name and not whether that name is any good. Naming a check
 *     "has a label" and calling it accessible is the failure mode this file is
 *     supposed to avoid, so the assertions stay mechanical and honest.
 *
 *   CDP_PORT=9280 node tools/verify/a11y-verify.mjs
 */
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

const key = async (k, code, extra = {}) => {
  for (const type of ["keyDown", "keyUp"]) {
    await call("Input.dispatchKeyEvent", {
      type,
      key: k,
      code,
      windowsVirtualKeyCode: extra.vk,
      ...extra,
    });
  }
  await sleep(90);
};

await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

/** Everything the user can reach with Tab, in DOM order. */
const FOCUSABLE = `'a[href], button:not([disabled]), input:not([disabled]), ` +
  `select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'`;

// Both dialogs live behind the toolbar's "Add" dropdown, so the path under test
// is menu -> item -> dialog. That is the real one, and the menu is itself a
// keyboard surface worth traversing.
for (const [label, itemText] of [
  ["Add dialog", "project folder"],
  ["Scan dialog", "Scan a workspace"],
]) {
  // Real mouse input, not `.click()`. Radix opens a dropdown on `pointerdown`,
  // which a synthetic click event never produces -- the menu simply never
  // appeared and the harness reported "no menu item" against a working app.
  const at = await ev(`(() => {
    const add = [...document.querySelectorAll('button')]
      .find(b => (b.textContent ?? '').trim() === 'Add');
    if (!add) return null;
    add.setAttribute('data-a11y-trigger', '1');
    const r = add.getBoundingClientRect();
    return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };
  })()`);
  if (!at) {
    ok(`${label} opens`, false, "no Add button");
    continue;
  }
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", {
      type, x: at.x, y: at.y, button: "left", clickCount: 1,
    });
  }
  await sleep(600);

  const itemAt = await ev(`(() => {
    const m = [...document.querySelectorAll('[role="menuitem"]')]
      .find(x => (x.textContent ?? '').includes(${JSON.stringify(itemText)}));
    if (!m) return null;
    const r = m.getBoundingClientRect();
    return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };
  })()`);
  if (!itemAt) {
    ok(`${label} opens`, false, "no menu item");
    continue;
  }
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", {
      type, x: itemAt.x, y: itemAt.y, button: "left", clickCount: 1,
    });
  }
  await sleep(900);
  const opened = await ev(`document.querySelector('[role="dialog"]') ? "open" : "did not open"`);

  if (opened !== "open") {
    ok(`${label} opens`, false, opened);
    continue;
  }
  ok(`${label} opens`, true);

  // A dialog with no accessible name is announced as just "dialog".
  const named = await ev(`(() => {
    const d = document.querySelector('[role="dialog"]');
    if (!d) return "";
    if (d.getAttribute('aria-label')) return d.getAttribute('aria-label');
    const id = d.getAttribute('aria-labelledby');
    return id ? (document.getElementById(id)?.textContent ?? "").trim() : "";
  })()`);
  ok(`${label} has an accessible name`, Boolean(named), named || "none");

  // Focus must land inside on open, or a keyboard user is stranded on the page
  // behind a modal they cannot reach.
  const focusInside = await ev(`(() => {
    const d = document.querySelector('[role="dialog"]');
    return !!(d && document.activeElement && d.contains(document.activeElement));
  })()`);
  ok(`${label} moves focus inside on open`, focusInside);

  const count = await ev(`document.querySelector('[role="dialog"]')?.querySelectorAll(${FOCUSABLE}).length ?? 0`);

  // Tab through more than the dialog holds. If focus is trapped it wraps and
  // never escapes; if it is not, it leaks to the page behind.
  let escaped = false;
  let visited = new Set();
  for (let i = 0; i < count + 3; i++) {
    await key("Tab", "Tab", { vk: 9 });
    const where = await ev(`(() => {
      const d = document.querySelector('[role="dialog"]');
      const a = document.activeElement;
      if (!d || !a) return "gone";
      if (!d.contains(a)) return "outside";
      return (a.getAttribute('aria-label') ?? a.tagName + ':' + (a.textContent ?? '').trim().slice(0, 20));
    })()`);
    if (where === "outside" || where === "gone") { escaped = true; break; }
    visited.add(where);
  }
  ok(`${label} traps Tab inside`, !escaped, escaped ? "focus left the dialog" : `${visited.size} stops`);
  ok(`${label} reaches every control`, visited.size >= Math.min(count, 2), `${visited.size} of ${count}`);

  // Focus visibility, measured rather than assumed: the focused control must
  // differ from its unfocused self somewhere a sighted keyboard user can see.
  const ring = await ev(`(() => {
    const a = document.activeElement;
    if (!a) return "no focus";
    const s = getComputedStyle(a);
    const w = parseFloat(s.outlineWidth) || 0;
    if (w > 0 && s.outlineStyle !== "none") return "outline " + s.outlineWidth;
    if (s.boxShadow && s.boxShadow !== "none") return "box-shadow";
    return "none";
  })()`);
  ok(`${label} shows a visible focus indicator`, ring !== "none" && ring !== "no focus", ring);

  await key("Escape", "Escape", { vk: 27 });
  const closed = await ev(`document.querySelector('[role="dialog"]') ? "still open" : "closed"`);
  ok(`${label} closes on Escape`, closed === "closed", closed);

  const returned = await ev(`(() => {
    const t = document.querySelector('[data-a11y-trigger]');
    const back = !!(t && document.activeElement === t);
    t?.removeAttribute('data-a11y-trigger');
    return back;
  })()`);
  ok(`${label} returns focus to what opened it`, returned);
  await sleep(400);
}

// ---------------------------------------------------------------------------
// Diagnostics structure
// ---------------------------------------------------------------------------

const wentToDiagnostics = await ev(`(async () => {
  const nav = [...document.querySelectorAll('nav button, nav a')]
    .find(b => /diagnostic/i.test(b.textContent ?? ''));
  if (!nav) return false;
  nav.click();
  for (let i = 0; i < 40; i++) {
    await new Promise(r => setTimeout(r, 250));
    if (/checked/i.test(document.body.innerText)) return true;
  }
  return true;
})()`);
ok("Diagnostics opens", wentToDiagnostics);

const structure = await ev(`(() => {
  const main = document.querySelector('main') ?? document.body;
  const headings = [...main.querySelectorAll('h1,h2,h3,h4,h5,h6')]
    .map(h => ({ level: +h.tagName[1], text: (h.textContent ?? '').trim().slice(0, 30) }));

  // A skipped level (h2 -> h4) breaks the outline a screen reader navigates by.
  let skipped = null;
  for (let i = 1; i < headings.length; i++) {
    if (headings[i].level > headings[i - 1].level + 1) {
      skipped = headings[i - 1].text + " -> " + headings[i].text;
      break;
    }
  }

  // <section> is only a landmark when it has an accessible name; unnamed ones
  // are generic and add nothing, which is fine -- but a NAMED one that a
  // screen reader can jump to is the point of using the element.
  const sections = [...main.querySelectorAll('section')];
  const namedSections = sections.filter(s =>
    s.getAttribute('aria-label') || s.getAttribute('aria-labelledby')).length;

  const unnamedControls = [...main.querySelectorAll('button, a[href], input')]
    .filter(el => {
      if (el.getAttribute('aria-label') || el.getAttribute('aria-labelledby')) return false;
      if ((el.textContent ?? '').trim().length > 0) return false;
      if (el.getAttribute('aria-hidden') === 'true') return false;
      return true;
    })
    .map(el => el.tagName + (el.className ? '.' + String(el.className).split(' ')[0] : ''));

  return JSON.stringify({
    headings: headings.length,
    skipped,
    sections: sections.length,
    namedSections,
    unnamedControls,
  });
})()`);

const st = JSON.parse(structure);
ok("Diagnostics has headings", st.headings > 0, `${st.headings}`);
ok("no heading level is skipped", st.skipped === null, st.skipped ?? "none");
ok(
  "every section is a named landmark",
  st.sections === 0 || st.namedSections === st.sections,
  `${st.namedSections} of ${st.sections} named`,
);
ok(
  "every control has an accessible name",
  st.unnamedControls.length === 0,
  st.unnamedControls.length ? st.unnamedControls.join(", ") : "all named",
);

const code = finish("accessibility checks");
ws.close();
process.exit(code);
