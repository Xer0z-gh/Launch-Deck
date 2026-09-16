/**
 * The launcher promises, checked against the real library.
 *
 * Tanner's brief of 2026-09-14 opened with "ENSURE EVERYTHING LAUNCHES JUST
 * FINE REGARDLESS OF LANGUAGE". These are the assertions behind that: a row
 * knows whether it can start BEFORE you press anything, it says why when it
 * cannot, the reason comes with a repair, and the list leads with what is
 * actually used rather than with whatever is alphabetically first.
 *
 * Every check runs against the shipped binary and the real registry -- 56
 * projects across 15 runners -- because a launcher that works on a fixture of
 * three is not the claim being made.
 *
 *   CDP_PORT=9280 node tools/verify/launcher-verify.mjs
 */
import { connect, reporter } from "./cdp.mjs";

const { ws, call, ev } = await connect();
const { ok, finish } = reporter();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

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
  await sleep(450);
  return true;
};

// The library must be what the app opens on.
await ev(`(async () => {
  for (let i = 0; i < 40; i++) {
    if (document.querySelector("[data-project-row]")) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);
ok(
  "the app opens on the library, ready to launch something",
  await ev(`!!document.querySelector("[data-project-row]")
            && !document.querySelector('section[aria-labelledby="dash-now"]')`),
);

// Readiness is one query for the whole library; let it land.
await sleep(2500);

// ---------------------------------------------- 1. verdicts before launching

const verdicts = await ev(`(() => {
  const rows = [...document.querySelectorAll("[data-project-row]")];
  const trouble = rows.filter(r => r.querySelector("p.text-signal"));
  return {
    total: rows.length,
    blocked: trouble.length,
    reasons: trouble.map(r => r.querySelector("p.text-signal").textContent.trim()),
  };
})()`);

ok("the library is populated", verdicts.total > 20, `${verdicts.total} rows`);
// Some must be blocked and some must not: a checker that flags everything is
// as useless as one that flags nothing, and both would pass a bare "> 0".
ok(
  "some projects report a blocker and most do not",
  verdicts.blocked > 0 && verdicts.blocked < verdicts.total / 2,
  `${verdicts.blocked} of ${verdicts.total} blocked`,
);

// Every reason has to name something specific. "Cannot start" is not a reason.
const vague = verdicts.reasons.filter(
  (r) => !/not installed|does not exist|folder is gone|not been built|Dependencies|virtual environment|Last run failed|binaries|Packaged/i.test(r),
);
ok(
  "every blocker names a specific, actionable cause",
  vague.length === 0,
  vague.length ? `vague: ${vague.join(" | ")}` : `${verdicts.reasons.length} specific reasons`,
);

// The three kinds of evidence this release added, each proven present by a
// real row rather than by the code path existing.
const kinds = {
  missingFolder: verdicts.reasons.some((r) => /folder is gone/i.test(r)),
  missingProgram: verdicts.reasons.some((r) => /is not installed, or not on PATH/i.test(r)),
  history: verdicts.reasons.some((r) => /Last run failed/i.test(r)),
};
ok("a project whose folder vanished is reported as such", kinds.missingFolder);
ok("a project whose toolchain is absent is reported before launching", kinds.missingProgram);
ok(
  "a runtime failure no static check could see is carried from run history",
  kinds.history,
);

// ------------------------------------------------- 2. the blocker is a repair
//
// EVERY blocked row must offer a way out, but not the same way out. A missing
// dependency keeps its one-press Install button -- that is the repair, and
// routing it through a dialog to press one button would be worse. Everything
// else opens the repair dialog. An earlier version of this check looked only
// for the dialog, found a dependency row first, and reported "no repair
// control" on a row that had one.
const repairs = await ev(`(() => {
  const rows = [...document.querySelectorAll("[data-project-row]")]
    .filter(r => r.querySelector("p.text-signal"));
  return rows.map(r => {
    const reason = r.querySelector("p.text-signal").textContent.trim();
    const labels = [...r.querySelectorAll("button")]
      .map(b => b.getAttribute("aria-label") ?? "");
    return {
      reason,
      hasRepair: labels.some(l => /cannot start/.test(l)),
      hasInstall: labels.some(l => /^Install dependencies/.test(l)),
      // "Last run failed" is history, not a present blocker: Run stays
      // offered, because the evidence says it broke, not that it cannot start.
      isHistory: /Last run failed/.test(reason),
      hasRun: labels.some(l => /^Run /.test(l)),
    };
  });
})()`);

const stranded = repairs.filter(
  (r) => !r.hasRepair && !r.hasInstall && !(r.isHistory && r.hasRun),
);
ok(
  "every blocked row offers a way out",
  stranded.length === 0,
  stranded.length ? `stranded: ${stranded.map((r) => r.reason).join(" | ")}` : `${repairs.length} rows`,
);
ok(
  "a dependency blocker keeps its one-press install rather than a dialog",
  repairs.some((r) => r.hasInstall && !r.hasRepair),
);
ok(
  "a history-only row still offers Run, because it is evidence not a block",
  repairs.filter((r) => r.isHistory).every((r) => r.hasRun),
);

// Now the dialog itself, opened from a row that genuinely cannot start.
const hardRow = `[...document.querySelectorAll("[data-project-row]")].find(r =>
  [...r.querySelectorAll("button")].some(b => /cannot start/.test(b.getAttribute("aria-label") ?? "")))`;
ok(
  "at least one row cannot start for a reason needing more than an install",
  await ev(`!!${hardRow}`),
);

const opened = await clickSel(`(() => {
  const row = ${hardRow};
  return row ? [...row.querySelectorAll("button")].find(b => /cannot start/.test(b.getAttribute("aria-label") ?? "")) : null;
})()`);
ok("the repair dialog opens", opened && (await ev(`!!document.querySelector('[role="dialog"]')`)));

const dialog = await ev(`(() => {
  const d = document.querySelector('[role="dialog"]');
  if (!d) return null;
  return {
    text: d.textContent,
    showsFolder: /Folder/.test(d.textContent),
    actions: [...d.querySelectorAll("button")].map(b => b.textContent.trim()).filter(Boolean),
  };
})()`);
ok("the repair dialog shows what it inspected", dialog?.showsFolder === true);
ok(
  "the repair dialog offers an action beyond Close",
  (dialog?.actions ?? []).filter((a) => a !== "Close").length > 0,
  (dialog?.actions ?? []).join(", "),
);
await call("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
await call("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
await sleep(400);

// --------------------------------------------- 3. the list leads with real use

const order = await ev(`(() => {
  const rows = [...document.querySelectorAll("[data-project-row]")];
  const text = rows.map(r => r.textContent);
  return {
    first10NeverRun: text.slice(0, 10).filter(t => /last run never/.test(t)).length,
    totalNeverRun: text.filter(t => /last run never/.test(t)).length,
  };
})()`);
// The whole point of the reorder: never-launched projects stop occupying the
// first screen. They are not hidden -- search still finds them.
ok(
  "never-launched projects are not on the first screen",
  order.first10NeverRun === 0,
  `${order.first10NeverRun} of the top 10, ${order.totalNeverRun} in the library`,
);
ok(
  "there are genuinely never-launched projects to have sorted down",
  order.totalNeverRun > 0,
  `${order.totalNeverRun}`,
);

// ------------------------------------------------ 4. launch options are real

const menuOpened = await clickSel(`(() => {
  const row = document.querySelector("[data-project-row]");
  return row ? [...row.querySelectorAll("button")].find(b => /More actions/.test(b.getAttribute("aria-label") ?? "")) : null;
})()`);
ok("the row menu opens", menuOpened);
await sleep(300);
const hasOptions = await ev(`
  [...document.querySelectorAll('[role="menuitem"]')].some(i => /Launch options/.test(i.textContent))
`);
ok("launch options are reachable from the row", hasOptions);

await clickSel(`[...document.querySelectorAll('[role="menuitem"]')].find(i => /Launch options/.test(i.textContent))`);
await sleep(600);
const options = await ev(`(() => {
  const d = document.querySelector('[role="dialog"]');
  if (!d) return null;
  return {
    showsPreview: /Will run/.test(d.textContent),
    hasCommandField: !!d.querySelector('input[aria-label="Command"]'),
    hasFlagField: !!d.querySelector('input[aria-label="New flag"]'),
  };
})()`);
ok("launch options show the command that will actually run", options?.showsPreview === true);
ok("the command is editable in the GUI", options?.hasCommandField === true);
ok("flags can be added in the GUI", options?.hasFlagField === true);

// Adding a flag must change the previewed command -- that is the whole claim.
const before = await ev(`document.querySelector('[role="dialog"]')?.querySelector("p.font-mono")?.textContent?.trim() ?? ""`);
await clickSel(`document.querySelector('input[aria-label="New flag"]')`);
for (const ch of "--verbose") {
  await call("Input.dispatchKeyEvent", { type: "keyDown", text: ch, key: ch });
  await call("Input.dispatchKeyEvent", { type: "keyUp", key: ch });
}
await clickSel(`[...document.querySelectorAll('[role="dialog"] button')].find(b => b.textContent.trim() === "Add")`);
await sleep(400);
const after = await ev(`document.querySelector('[role="dialog"]')?.querySelector("p.font-mono")?.textContent?.trim() ?? ""`);
ok(
  "adding a flag updates the previewed command",
  after.includes("--verbose") && after !== before,
  `${before} -> ${after}`,
);

// And switching it off must take it back out, WITHOUT deleting it.
await clickSel(`document.querySelector('[role="dialog"] input[type="checkbox"]')`);
await sleep(400);
const toggled = await ev(`(() => {
  const d = document.querySelector('[role="dialog"]');
  return {
    preview: d?.querySelector("p.font-mono")?.textContent?.trim() ?? "",
    stillListed: /--verbose/.test(d?.textContent ?? ""),
  };
})()`);
ok(
  "disabling a flag removes it from the command but keeps it listed",
  !toggled.preview.includes("--verbose") && toggled.stillListed,
  `preview="${toggled.preview}" listed=${toggled.stillListed}`,
);

// Cancel: nothing above should have been saved.
await clickSel(`[...document.querySelectorAll('[role="dialog"] button')].find(b => b.textContent.trim() === "Cancel")`);
await sleep(400);
ok("the dialog closes on cancel", await ev(`!document.querySelector('[role="dialog"]')`));

// ------------------------------------- 5. things that are not whole projects
//
// The brief asked for folders, files, scripts and commands as first-class
// launcher items. The underlying support already existed -- `register_project`
// has always taken a custom command -- so what is checked here is the front
// door, which is what was missing.
//
// NOT checked, and it cannot be from here: the native file picker. Choosing a
// file opens an OS dialog that CDP cannot drive, so the proposal step
// (`.ps1` -> powershell, `.py` -> python, anything else -> the shell) is
// exercised by hand rather than asserted. The dialog shows the proposed
// command in an editable field before saving, so a wrong guess is visible
// rather than silent.
await clickSel(`[...document.querySelectorAll("button")].find(b => b.textContent.trim() === "Add")`);
await sleep(400);
const addMenu = await ev(`
  [...document.querySelectorAll('[role="menuitem"]')].map(i => i.textContent.trim())
`);
ok(
  "the Add menu names what can be added, not just folders",
  addMenu.some(i => /file, script or folder/i.test(i)) && addMenu.some(i => /web app/i.test(i)),
  addMenu.join(" | "),
);

await clickSel(`[...document.querySelectorAll('[role="menuitem"]')].find(i => /file, script or folder/i.test(i.textContent))`);
await sleep(600);
const shortcut = await ev(`(() => {
  const d = document.querySelector('[role="dialog"]');
  if (!d) return null;
  const buttons = [...d.querySelectorAll("button")].map(b => b.textContent.trim());
  return {
    picksFile: buttons.some(b => /Choose a file/.test(b)),
    picksFolder: buttons.some(b => /Choose a folder/.test(b)),
    showsCommand: !!d.querySelector('input[aria-label="Command"]'),
    // Nothing can be added until something is chosen, so a blank entry
    // cannot be created by pressing Add twice.
    addDisabled: [...d.querySelectorAll("button")].find(b => b.textContent.trim() === "Add")?.disabled,
  };
})()`);
ok("a file or a folder can be chosen", shortcut?.picksFile === true && shortcut?.picksFolder === true);
ok("the command that will run is shown and editable", shortcut?.showsCommand === true);
ok("nothing can be added before a target is chosen", shortcut?.addDisabled === true);

await call("Input.dispatchKeyEvent", { type: "keyDown", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });
await call("Input.dispatchKeyEvent", { type: "keyUp", key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 });

ws.close();
finish("launcher");
