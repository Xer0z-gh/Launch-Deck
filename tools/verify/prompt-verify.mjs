/**
 * Proves Prompt Studio end to end against files this harness owns.
 *
 * The Launch-Deck row is the guinea pig, so the card and hand-off writes land
 * in THIS repo's root — the suite snapshots any pre-existing LAUNCHDECK.md /
 * NEXT.md first and restores them (or deletes its own) at the end, and its
 * final check asserts the cleanup actually happened. If the suite dies midway
 * the leftovers show up in `git status`, which is the desired failure mode:
 * visible, not silent.
 *
 * What it walks, all through the real UI: menu → studio, template radios,
 * intent typing, honest no-card wording, card create/save (asserted on disk),
 * prompt preview embedding the saved card, NEXT.md hand-off (append proven by
 * doing it twice), and the "Debug this…" preset. `open_claude_terminal` is
 * deliberately NOT here — a suite that pops terminal windows on every run
 * would train the operator to stop running it.
 *
 *   CDP_PORT=9280 node tools/verify/prompt-verify.mjs
 */
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

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

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const CARD = path.join(repoRoot, "LAUNCHDECK.md");
const NEXT = path.join(repoRoot, "NEXT.md");
const saved = new Map();
for (const f of [CARD, NEXT]) {
  if (existsSync(f)) saved.set(f, readFileSync(f, "utf8"));
}
const restore = () => {
  for (const f of [CARD, NEXT]) {
    const before = saved.get(f);
    if (before === undefined) rmSync(f, { force: true });
    else writeFileSync(f, before);
  }
};

const TARGET = "Launch-Deck";

const click = async (x, y) => {
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
};
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
const menuItem = async (label) => {
  await clickSel(`document.querySelector('[aria-label="More actions for ${TARGET}"]')`);
  // Poll for the menu rather than trusting a fixed delay -- right after boot
  // the app is still priming queries and the menu can take longer to mount.
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
  const hit = await clickSel(
    `[...document.querySelectorAll('[role="menuitem"]')].find(m => (m.textContent ?? '').includes('${label}'))`,
  );
  await sleep(700);
  return hit;
};
const setReactValue = (selExpr, value, proto = "HTMLInputElement") => ev(`(() => {
  const el = ${selExpr};
  if (!el) return false;
  const setter = Object.getOwnPropertyDescriptor(window.${proto}.prototype, "value").set;
  setter.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event("input", { bubbles: true }));
  return true;
})()`);
const preview = () =>
  ev(`document.querySelector('[aria-label="Prompt Studio for ${TARGET}"] pre')?.textContent ?? ""`);

/**
 * Waits for the preview to satisfy `test`. The card query serves cached data
 * while it refetches, so immediately after (re)opening the studio the preview
 * can briefly reflect a previous state — asserting through this helper means
 * asserting the settled truth, not the race.
 */
const previewSettles = async (test) => {
  let text = "";
  for (let i = 0; i < 24; i++) {
    text = await preview();
    if (test(text)) return true;
    await sleep(250);
  }
  return false;
};

// Toasts pop bottom-right, exactly over the studio's footer buttons, and a
// click that lands on a toast is a click the button never saw (toasts live
// 6 s). Every footer click waits them out first.
const awaitNoToasts = () => ev(`(async () => {
  for (let i = 0; i < 40; i++) {
    if (document.querySelectorAll('[role="status"], [role="alert"]').length === 0) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);

try {
  await ev(`(async () => {
    for (let i = 0; i < 80; i++) {
      if (document.querySelectorAll('[data-project-row]').length > 0) return true;
      await new Promise(r => setTimeout(r, 250));
    }
    return false;
  })()`);

  // 1. Menu opens the studio.
  ok("Build prompt… menu item exists", await menuItem("Build prompt"));
  ok(
    "studio panel opens",
    await ev(`!!document.querySelector('[aria-label="Prompt Studio for ${TARGET}"]')`),
  );
  ok(
    "focus moves into the studio's intent input on open",
    await ev(`document.activeElement === document.querySelector('[aria-label="Prompt Studio for ${TARGET}"] input')`),
  );

  // 2. Template picker is a real radiogroup with Feature preselected.
  ok(
    "six templates as radios, Feature preselected",
    await ev(`(() => {
      const radios = [...document.querySelectorAll('[role="radiogroup"][aria-label="Task template"] [role="radio"]')];
      const checked = radios.find(r => r.getAttribute('aria-checked') === 'true');
      return radios.length === 6 && checked?.textContent === 'Feature';
    })()`),
  );

  // 2b. The radio contract is real: one Tab stop, arrows move selection.
  ok(
    "only the checked pill is in the Tab order",
    await ev(`(() => {
      const radios = [...document.querySelectorAll('[role="radiogroup"][aria-label="Task template"] [role="radio"]')];
      const stops = radios.filter(r => r.tabIndex === 0);
      return stops.length === 1 && stops[0].getAttribute('aria-checked') === 'true';
    })()`),
  );
  await ev(`document.querySelector('[role="radiogroup"][aria-label="Task template"] [role="radio"][aria-checked="true"]').focus()`);
  for (const type of ["keyDown", "keyUp"]) {
    await call("Input.dispatchKeyEvent", {
      type, key: "ArrowRight", code: "ArrowRight",
      windowsVirtualKeyCode: 39, nativeVirtualKeyCode: 39,
    });
  }
  await sleep(300);
  ok(
    "ArrowRight moves the template selection",
    await ev(`document.querySelector('[role="radiogroup"][aria-label="Task template"] [role="radio"][aria-checked="true"]')?.textContent !== 'Feature'`),
  );
  // Back to Feature so the rest of the walk sees the default template.
  await clickSel(`[...document.querySelectorAll('[role="radio"]')].find(r => r.textContent === 'Feature')`);
  await sleep(200);

  // 2c. The preview is keyboard-scrollable (focusable region).
  ok(
    "prompt preview is focusable for keyboard scrolling",
    await ev(`document.querySelector('[aria-label="Prompt Studio for ${TARGET}"] pre')?.tabIndex === 0`),
  );

  // 3. Intent lands in the preview verbatim.
  const INTENT = "Wire the frobnicator to the main panel.";
  await setReactValue(`document.querySelector('[aria-label="Prompt Studio for ${TARGET}"] input')`, INTENT);
  await sleep(300);
  ok("typed intent appears in the preview", (await preview()).includes(INTENT));

  // 4. No card on disk -> the prompt says so, honestly, and carries the footer.
  ok(
    "missing card is stated, not faked",
    await previewSettles((t) => t.includes("no context card yet")),
  );
  ok("vault footer is present", (await preview()).includes("ClaudeVault"));

  // 5. Create the card through the editor; assert the file and the preview.
  await clickSel(
    `[...document.querySelectorAll('[aria-label="Prompt Studio for ${TARGET}"] button')].find(b => b.textContent.includes('Create card'))`,
  );
  await sleep(300);
  const CARD_BODY = "# LAUNCHDECK.md\n\n## Stack\n\nRust backend, React front, test card.";
  ok(
    "card editor accepts input",
    await setReactValue(
      `document.querySelector('[aria-label="Context card content"]')`,
      CARD_BODY,
      "HTMLTextAreaElement",
    ),
  );
  await clickSel(
    `[...document.querySelectorAll('[aria-label="Prompt Studio for ${TARGET}"] button')].find(b => b.textContent === 'Save card')`,
  );
  await sleep(900);
  ok(
    "card saved to the repo root on disk",
    existsSync(CARD) && readFileSync(CARD, "utf8").includes("test card"),
  );
  ok(
    "saved card is embedded in the preview",
    await previewSettles((t) => t.includes("test card")),
  );

  // 6. Hand-off twice: NEXT.md exists, and the second write APPENDS.
  await awaitNoToasts();
  ok(
    "hand-off button clicked",
    await clickSel(
      `[...document.querySelectorAll('[aria-label="Prompt Studio for ${TARGET}"] button')].find(b => b.textContent.includes('NEXT.md'))`,
    ),
  );
  await sleep(900);
  const first = existsSync(NEXT) ? readFileSync(NEXT, "utf8") : "";
  ok(
    "hand-off writes NEXT.md with the prompt",
    first.includes("Handed off from Launch Deck") && first.includes(INTENT),
  );
  await awaitNoToasts();
  await clickSel(
    `[...document.querySelectorAll('[aria-label="Prompt Studio for ${TARGET}"] button')].find(b => b.textContent.includes('NEXT.md'))`,
  );
  await sleep(900);
  const second = readFileSync(NEXT, "utf8");
  ok(
    "second hand-off appends instead of replacing",
    second.startsWith(first.trimEnd()) &&
      (second.match(/Handed off from Launch Deck/g) ?? []).length === 2,
  );

  // 7. "Debug this…" preselects the debug template.
  await clickSel(
    `document.querySelector('[aria-label="Prompt Studio for ${TARGET}"] [aria-label="Close Prompt Studio"]')`,
  );
  await sleep(600);
  ok("Debug this… menu item exists", await menuItem("Debug this"));
  ok(
    "debug preset selects the Debug template",
    await ev(`(() => {
      const checked = document.querySelector('[role="radiogroup"][aria-label="Task template"] [role="radio"][aria-checked="true"]');
      return checked?.textContent === 'Debug';
    })()`),
  );

  // 8. Esc peels ONE layer per press: card editor -> studio -> selection.
  // Close the debug studio first: while a panel is open at a narrow window
  // the main column is (by design) hidden, and the row cannot be clicked.
  await clickSel(
    `document.querySelector('[aria-label="Prompt Studio for ${TARGET}"] [aria-label="Close Prompt Studio"]')`,
  );
  await sleep(600);
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
  if (rowAt) await click(rowAt.x, rowAt.y);
  await sleep(600);
  // Reopen the studio on top of the selection, then the card editor on top
  // of that -- three layers up.
  await menuItem("Build prompt");
  await clickSel(
    `[...document.querySelectorAll('[aria-label="Prompt Studio for ${TARGET}"] button')].find(b => b.textContent.includes('Edit card') || b.textContent.includes('Create card'))`,
  );
  await sleep(400);
  const layers = () => ev(`JSON.stringify({
    editor: !!document.querySelector('[aria-label="Context card content"]'),
    studio: !!document.querySelector('[aria-label="Prompt Studio for ${TARGET}"].is-open'),
    bar: !!document.querySelector('footer.bar-enter.is-open'),
  })`);
  const esc = async () => {
    for (const type of ["keyDown", "keyUp"]) {
      await call("Input.dispatchKeyEvent", {
        type, key: "Escape", code: "Escape",
        windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27,
      });
    }
    await sleep(450);
  };
  await esc();
  let state = JSON.parse(await layers());
  ok(
    "first Esc closes only the card editor",
    !state.editor && state.studio && state.bar,
    JSON.stringify(state),
  );
  await esc();
  state = JSON.parse(await layers());
  ok(
    "second Esc closes only the studio",
    !state.studio && state.bar,
    JSON.stringify(state),
  );
  await esc();
  state = JSON.parse(await layers());
  ok("third Esc clears the selection", !state.bar, JSON.stringify(state));
} finally {
  restore();
}

ok(
  "harness restored the repo files it touched",
  existsSync(CARD) === saved.has(CARD) && existsSync(NEXT) === saved.has(NEXT),
);

const code = finish("prompt studio checks");
ws.close();
process.exit(code);
