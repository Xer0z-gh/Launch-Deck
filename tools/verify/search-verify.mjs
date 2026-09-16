/**
 * Proves ranked search and the Today strip against the real library.
 *
 * Search assertions pin the RANKING contract, not just filtering: a name
 * prefix beats a path hit, a subsequence still finds its target, Enter takes
 * the top row. The library's actual contents are the fixture — these names
 * (Fleet, Launch-Deck, Pulse) are real registered projects, and if one is
 * ever removed the failure message says exactly that.
 *
 * The Today strip is asserted structurally and for internal consistency
 * (its running count must agree with the table's live rows) — the suite
 * cannot fabricate an overnight death without polluting real run history,
 * so the death path is covered by the store test
 * `failures_since_windows_on_death_time_across_projects` instead.
 *
 *   CDP_PORT=9280 node tools/verify/search-verify.mjs
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

await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);

const setQuery = async (value) => {
  await ev(`(() => {
    const el = document.querySelector('input[aria-label="Search projects"]');
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    setter.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event("input", { bubbles: true }));
  })()`);
  await sleep(350);
};
const rowNames = () =>
  ev(`JSON.stringify([...document.querySelectorAll('[aria-label^="More actions for "]')].map(b => b.getAttribute('aria-label').replace('More actions for ', '')))`);

const total = JSON.parse(await rowNames()).length;
ok("library renders", total > 10, `${total} rows`);

// 1. Name prefix outranks everything: "fl" -> Fleet first even though many
// projects live under paths containing the letters.
await setQuery("fl");
let names = JSON.parse(await rowNames());
ok("name prefix ranks first (fl -> Fleet)", names[0] === "Fleet", names.slice(0, 3).join(", "));

// 2. Subsequence finds a name substring search would miss.
await setQuery("lchdck");
names = JSON.parse(await rowNames());
ok(
  "subsequence match finds Launch-Deck from 'lchdck'",
  names.includes("Launch-Deck"),
  names.slice(0, 3).join(", ") || "no rows",
);

// 3. Multi-word: every term must match; name term dominates the ranking.
await setQuery("pulse rust");
names = JSON.parse(await rowNames());
ok(
  "multi-word AND ranks Pulse first for 'pulse rust'",
  names[0] === "Pulse",
  names.slice(0, 3).join(", ") || "no rows",
);

// 4. Enter takes the top hit: the detail bar opens on it.
await setQuery("fleet");
await ev(`document.querySelector('input[aria-label="Search projects"]').focus()`);
for (const type of ["keyDown", "keyUp"]) {
  await call("Input.dispatchKeyEvent", {
    type, key: "Enter", code: "Enter",
    windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13,
  });
}
await sleep(700);
ok(
  "Enter selects the top hit",
  await ev(`document.querySelector('footer.bar-enter h2')?.textContent === 'Fleet'`),
);
// Clear the selection again (Esc peels: nothing else is open).
for (const type of ["keyDown", "keyUp"]) {
  await call("Input.dispatchKeyEvent", {
    type, key: "Escape", code: "Escape",
    windowsVirtualKeyCode: 27, nativeVirtualKeyCode: 27,
  });
}
await sleep(400);

// 5. Clearing restores the full library.
await setQuery("");
names = JSON.parse(await rowNames());
ok("clearing the query restores every row", names.length === total, `${names.length}/${total}`);

// 6. The Today strip: present, labeled, and self-consistent.
ok("Today strip renders", await ev(`!!document.querySelector('[aria-label="Today"]')`));
const strip = await ev(`document.querySelector('[aria-label="Today"]')?.textContent ?? ""`);
ok("strip states facts, not blanks", strip.trim().length > 10, strip.slice(0, 60));
const liveRows = await ev(
  `document.querySelectorAll('[data-project-row][data-live]').length`,
);
const claimsRunning = /(\d+) running/.exec(strip);
const stripCount = claimsRunning ? Number(claimsRunning[1]) : 0;
ok(
  "strip's running count agrees with the table",
  // Strict equality, both directions: zero must say "none running", and any
  // positive claim must match the table's live-edge count exactly.
  stripCount === liveRows && (stripCount > 0 || /none running/.test(strip)),
  `strip says ${claimsRunning ? stripCount : "none"}, table shows ${liveRows} live edge(s)`,
);

const code = finish("search + today checks");
ws.close();
process.exit(code);
