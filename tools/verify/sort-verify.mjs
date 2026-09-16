/**
 * Verifies sorting against the shipped build, through the TOOLBAR SORT MENU.
 *
 * The library became a grouped list, which has no header row to click, so the
 * control moved to the toolbar the way Files, Photos and Mail do it. What is
 * asserted did not change: the ROW ORDER actually changes and matches an
 * independent sort of the same values -- never merely that a checkmark moved.
 */
const PORT = process.env.CDP_PORT ?? "9310";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json())
  .find((t) => t.type === "page" && t.webSocketDebuggerUrl);
if (!page) { console.error("FAIL: no CDP target"); process.exit(1); }
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r, j) => ((ws.onopen = r), (ws.onerror = j)));
let id = 0;
const call = (m, p) => new Promise((res, rej) => {
  const mid = ++id;
  const t = setTimeout(() => rej(new Error(`${m} timed out`)), 20000);
  const h = (e) => { const d = JSON.parse(e.data); if (d.id === mid) { clearTimeout(t); ws.removeEventListener("message", h); res(d.result); } };
  ws.addEventListener("message", h);
  ws.send(JSON.stringify({ id: mid, method: m, params: p }));
});
const ev = async (x) => {
  const r = await call("Runtime.evaluate", { expression: x, returnByValue: true, awaitPromise: true });
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
await call("Emulation.setDeviceMetricsOverride", { width: 1600, height: 950, deviceScaleFactor: 1, mobile: false });
await sleep(800);

const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok });
  console.log(`  ${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
};

for (let i = 0; i < 40; i++) {
  if (await ev(`document.querySelectorAll("[data-project-row]").length`)) break;
  await sleep(400);
}

/** Cheap identity for the current row order, for change detection. */
const rowOrder = () => ev(`[...document.querySelectorAll("[data-project-row]")]
  .map((r) => r.getAttribute("data-project-id") ?? r.textContent ?? "").join("|")`);

/** Every row's project name, in render order. */
const names = () => ev(`[...document.querySelectorAll("[data-project-row]")]
  .map((r) => r.querySelector("span.truncate")?.textContent?.trim() ?? "")`);

/**
 * Real mouse input, not `.click()`.
 *
 * Radix opens a dropdown on POINTERDOWN. A synthetic `click()` dispatches a
 * click with no pointer sequence in front of it, so the menu never opens and
 * every later assertion reads a closed menu -- which is exactly how this
 * suite first reported "0 marked" against a working control.
 */
const mouse = async (x, y) => {
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", {
      type, x, y, button: "left", clickCount: 1,
    });
  }
};

const rectOf = (selector) => ev(`(() => {
  const el = ${selector};
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return r.width ? { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) } : null;
})()`);

/** Opens the sort menu and waits for its content to stop moving. */
const openSortMenu = async () => {
  const at = await rectOf(`document.querySelector('[data-sort-menu]')`);
  if (!at) return false;
  await mouse(at.x, at.y);
  const settled = await ev(`(async () => {
    const rect = () => JSON.stringify(
      document.querySelector('[role="menuitem"]')?.getBoundingClientRect() ?? null);
    let prev = null;
    for (let i = 0; i < 25; i++) {
      await new Promise(r => setTimeout(r, 80));
      const now = rect();
      if (now !== null && now === prev) return true;
      prev = now;
    }
    return false;
  })()`);
  return settled;
};

const closeMenu = async () => {
  for (const type of ["keyDown", "keyUp"]) {
    await call("Input.dispatchKeyEvent", {
      type, key: "Escape", code: "Escape",
      windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27,
    });
  }
  await sleep(250);
};

/**
 * Picks an option from the toolbar sort menu. Returns false if the menu or
 * the item never appeared, so a silently-inert control cannot pass.
 */
const pickSort = async (label) => {
  const before = await rowOrder();
  if (!(await openSortMenu())) return false;
  const at = await rectOf(
    `[...document.querySelectorAll('[role="menuitem"]')]` +
    `.find((m) => (m.textContent ?? "").trim().toLowerCase() === ${JSON.stringify(label.toLowerCase())})`,
  );
  if (!at) { await closeMenu(); return false; }
  await mouse(at.x, at.y);
  // Wait for React to commit rather than sleeping a fixed amount: under load
  // a fixed delay read the pre-commit order and reported failures against a
  // build that passes when run alone.
  for (let i = 0; i < 40; i++) {
    if ((await rowOrder()) !== before) return true;
    await sleep(50);
  }
  // A sort with ties can legitimately produce the same order; the caller's
  // own assertion is the real check.
  return true;
};

/** Which option the menu currently marks, read with the menu really open. */
const checkedSort = async () => {
  if (!(await openSortMenu())) return { label: null, markedCount: -1 };
  const state = await ev(`(() => {
    const items = [...document.querySelectorAll('[role="menuitem"]')];
    const marked = items.filter((m) => m.querySelector("svg"));
    return { label: marked.length === 1 ? marked[0].textContent.trim() : null, markedCount: marked.length };
  })()`);
  await closeMenu();
  return state;
};

// --- Name: the one order we can verify independently ------------------------
const before = await names();
check("list rendered", before.length > 5, `${before.length} rows`);

check("the toolbar sort menu opens and picks Name", await pickSort("Name"));
const asc = await names();
const sortedAsc = [...asc].sort((a, b) => a.toLocaleLowerCase().localeCompare(b.toLocaleLowerCase()));
check("ascending sort matches an independent sort of the same names",
  asc.length > 5 && asc.every((n) => n.length > 0)
    && JSON.stringify(asc) === JSON.stringify(sortedAsc),
  asc.slice(0, 3).join(" | "));

const markedAsc = await checkedSort();
check("the menu marks exactly one option, and it is Name",
  markedAsc.markedCount === 1 && markedAsc.label === "Name",
  `${markedAsc.markedCount} marked: ${markedAsc.label}`);

// Picking the same key again reverses, which is what the header's second
// click used to do.
await pickSort("Name");
const desc = await names();
check("choosing the same key again reverses the order",
  JSON.stringify(desc) === JSON.stringify([...asc].reverse()), desc.slice(0, 3).join(" | "));

// --- Default order is reachable explicitly ----------------------------------
await pickSort("Default order");
const cleared = await names();
check("Default order returns the list to its unsorted order",
  cleared.every((n) => n.length > 0) && JSON.stringify(cleared) === JSON.stringify(before),
  cleared.slice(0, 3).join(" | "));
const markedDefault = await checkedSort();
check("with no sort, the menu marks Default order",
  markedDefault.label === "Default order", markedDefault.label ?? "nothing marked");

// --- Another key actually reorders ------------------------------------------
check("sorting by Added is offered and changes the order",
  (await pickSort("Added")) && JSON.stringify(await names()) !== JSON.stringify(cleared));

// --- Persistence -------------------------------------------------------------
await pickSort("Kind");
const persisted = await ev(`localStorage.getItem("deck.sort.v1")`);
check("sort is persisted for the next launch", persisted !== null, persisted ?? "missing");

await pickSort("Default order");
check("cleared sort removes the persisted key",
  (await ev(`localStorage.getItem("deck.sort.v1")`)) === null);

// --- One selection at a time --------------------------------------------------
await pickSort("Status");
const only = await checkedSort();
check("only one option is ever marked", only.markedCount === 1, `${only.markedCount}`);
await pickSort("Default order");

await call("Emulation.clearDeviceMetricsOverride");
const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} sort checks passed`);
if (failed.length) console.log("failing:", failed.map((f) => f.name).join("; "));
process.exit(failed.length === 0 ? 0 : 1);
