/**
 * The state of the library itself: dead roots, duplicates, missing icons.
 *
 * `launch-all` answers "does it run". This answers the questions before that --
 * whether the row should exist at all, and whether it looks like anything.
 *
 * Reports only. Removing a project is destructive and the list is worth reading
 * before anything acts on it, so the acting is a separate deliberate step.
 *
 *   CDP_PORT=9280 node tools/verify/library-audit.mjs
 */
import { connect } from "./cdp.mjs";

const { ws, ev } = await connect();

await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);

// Widen the window so every column renders; `Root` is behind a container query
// and the audit needs the path, not just the name.
const rows = await ev(`JSON.stringify([...document.querySelectorAll('[data-project-row]')].map(tr => {
  const btn = tr.querySelector('button[aria-label^="Run "], button[aria-label^="Install dependencies for "]');
  const label = btn ? btn.getAttribute('aria-label') : '';
  const name = label.startsWith('Run ') ? label.slice(4) : label.slice(25);
  return {
    name,
    // A real icon is an <img>; the runner glyph fallback is an inline <svg>
    // inside a tile span, so the two are distinguishable without guessing.
    hasIcon: !!tr.querySelector('img'),
    // Run disabled with no Install offered is how a missing root presents.
    runDisabled: !!tr.querySelector('button[aria-label^="Run "][disabled]'),
    needsInstall: !!tr.querySelector('button[aria-label^="Install dependencies for "]'),
    text: tr.innerText.replace(/\\s+/g, ' ').trim(),
  };
}))`);

const list = JSON.parse(rows);
console.log(`${list.length} projects\n`);

const byName = new Map();
for (const p of list) byName.set(p.name, (byName.get(p.name) ?? 0) + 1);
const duplicateNames = [...byName.entries()].filter(([, n]) => n > 1);

const noIcon = list.filter((p) => !p.hasIcon);
const deadRoot = list.filter((p) => p.runDisabled && !p.needsInstall);

console.log(`duplicate names:   ${duplicateNames.length}`);
for (const [name, n] of duplicateNames) console.log(`  ${n}x  ${name}`);

console.log(`\nno real icon:      ${noIcon.length}`);
for (const p of noIcon) console.log(`  ${p.name}`);

console.log(`\nlikely dead root:  ${deadRoot.length}`);
for (const p of deadRoot) console.log(`  ${p.name}`);

ws.close();
