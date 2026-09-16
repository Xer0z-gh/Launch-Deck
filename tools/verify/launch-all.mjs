/**
 * Launches every project in turn and reports which ones actually work.
 *
 * "Some programs are broken and don't launch" is a claim about thirty projects,
 * and reading thirty runner manifests to guess which is the slow way to be
 * wrong. This presses Run on each one, waits, and records what the app itself
 * ends up believing plus the tail of the project's own log.
 *
 * Each project is stopped before the next starts, so a port conflict or a CPU
 * spike from one cannot be blamed on another.
 *
 * Reports, does not assert. The output is a triage list -- some failures will
 * be genuinely broken projects rather than wrong commands, and that difference
 * is a judgement call for a human reading the logs.
 *
 *   CDP_PORT=9280 node tools/verify/launch-all.mjs [settleMs]
 */
import { connect } from "./cdp.mjs";

const SETTLE = Number(process.argv[2] ?? 5000);
const { ws, ev } = await connect();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await ev(`(async () => {
  for (let i = 0; i < 80; i++) {
    if (document.querySelectorAll('[data-project-row]').length > 0) return true;
    await new Promise(r => setTimeout(r, 250));
  }
  return false;
})()`);

const projects = await ev(`[...document.querySelectorAll('[data-project-row]')].map(tr => {
  const run = tr.querySelector('button[aria-label^="Run "]');
  const install = tr.querySelector('button[aria-label^="Install dependencies for "]');
  return {
    name: run ? run.getAttribute('aria-label').slice(4)
              : install ? install.getAttribute('aria-label').slice(25) : null,
    runnable: !!(run && !run.disabled),
    needsInstall: !!install,
  };
}).filter(p => p.name)`);

console.log(`${projects.length} projects, ${projects.filter((p) => p.runnable).length} runnable\n`);

const results = [];
for (const p of projects) {
  if (!p.runnable) {
    results.push({ ...p, verdict: p.needsInstall ? "NEEDS INSTALL" : "NOT RUNNABLE", detail: "" });
    continue;
  }

  const started = await ev(`(() => {
    const b = document.querySelector('[aria-label="Run ' + ${JSON.stringify(p.name)} + '"]');
    if (!b || b.disabled) return false;
    b.scrollIntoView({ block: "center" });
    b.click();
    return true;
  })()`);
  if (!started) {
    results.push({ ...p, verdict: "NO BUTTON", detail: "" });
    continue;
  }

  await sleep(SETTLE);

  // The badge is the app's own verdict; the row text carries it.
  const state = await ev(`(() => {
    const b = document.querySelector('[aria-label="Run ' + ${JSON.stringify(p.name)} + '"], [aria-label="Stop ' + ${JSON.stringify(p.name)} + '"]');
    const tr = b && b.closest('tr');
    if (!tr) return "gone";
    const txt = tr.innerText.replace(/\\s+/g, " ");
    for (const s of ["Crashed", "Running", "Starting", "Stopping", "Exited", "Idle"]) {
      if (txt.includes(s)) return s;
    }
    return txt.slice(0, 40);
  })()`);

  // Open the log panel and take the tail, which is where a failure explains
  // itself -- the state alone says "Crashed" and never says why.
  const log = await ev(`(async () => {
    const b = document.querySelector('[aria-label="Logs for ' + ${JSON.stringify(p.name)} + '"]');
    if (!b) return "";
    b.click();
    await new Promise(r => setTimeout(r, 900));
    const panel = document.querySelector('aside.panel-enter');
    if (!panel) return "";
    const lines = panel.innerText.split("\\n").map(s => s.trim()).filter(Boolean);
    return lines.slice(-4).join(" | ").slice(0, 260);
  })()`);

  await ev(`document.querySelector('aside.panel-enter [aria-label="Close log panel"]')?.click()`);
  await sleep(300);

  // A one-shot program that finishes is not broken. Number-Remover, the
  // Claude-Auto-Resume installer and four others all exit 0 inside the
  // settle window, and listing them as failures buries the projects that
  // really are broken.
  //
  // Two separate things stopped that test working, and neither was visible
  // in the output. An earlier edit left a literal control byte in this
  // regex where a word-boundary escape was meant, so it matched nothing at
  // all. And the log panel's innerText carries non-breaking spaces from its
  // formatted layout, so a plain "code 0" would not have matched even
  // without that. The result was six clean exits reported as failures with
  // "Exited with code 0" printed directly underneath them -- output that
  // contradicts itself, which is the kind that gets a whole report ignored.
  const cleanExit =
    state === "Exited" && /code\s+0(\s|$)/.test(log.replace(/\s+/g, " "));
  const verdict =
    state === "Running" || state === "Starting" || cleanExit
      ? "ok"
      : state.toUpperCase();
  results.push({ ...p, verdict, detail: log });

  await ev(`document.querySelector('[aria-label="Stop ' + ${JSON.stringify(p.name)} + '"]')?.click()`);
  await sleep(900);
}

const bad = results.filter((r) => r.verdict !== "ok");
console.log(`${results.length - bad.length} launched, ${bad.length} did not\n`);
for (const r of bad) {
  console.log(`${r.verdict.padEnd(14)} ${r.name}`);
  if (r.detail) console.log(`               ${r.detail}`);
}

ws.close();
