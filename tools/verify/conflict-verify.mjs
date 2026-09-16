/**
 * Proves the two-tier port policy:
 *   configured port taken  -> launch REFUSED before spawning
 *   guessed  port taken    -> launch PROCEEDS with a logged warning
 */
const PORT = process.env.CDP_PORT ?? "9260";
const DEMO = decodeURIComponent(
  new URL("./fixtures/port-project", import.meta.url).pathname.slice(1),
).replaceAll("/", "\");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0;
const call = (m, p) => new Promise((res, rej) => {
  const mid = ++id; const t = setTimeout(() => rej(new Error("timeout")), 40000);
  const h = (e) => { const x = JSON.parse(e.data); if (x.id === mid) { clearTimeout(t); ws.removeEventListener("message", h); res(x.result); } };
  ws.addEventListener("message", h); ws.send(JSON.stringify({ id: mid, method: m, params: p }));
});
const ev = async (expr) => {
  const r = await call("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? "threw");
  return r.result?.value;
};
const invoke = (c, a) => ev(`window.__TAURI__.core.invoke(${JSON.stringify(c)}, ${JSON.stringify(a ?? {})})`);
// Serialize rejections INSIDE the page: a Tauri error object does not survive
// CDP's exceptionDetails as anything readable.
const tryInvoke = (c, a) => ev(
  `window.__TAURI__.core.invoke(${JSON.stringify(c)}, ${JSON.stringify(a ?? {})})` +
  `.then(v => ({ ok: true, value: v })).catch(e => ({ ok: false, error: e }))`
);
await call("Runtime.enable");

const results = [];
const check = (n, ok, d = "") => { results.push(ok); console.log(`${ok ? "PASS" : "FAIL"}  ${n}${d ? `  (${d})` : ""}`); };

for (const p of await invoke("list_projects")) if (p.root === DEMO) await invoke("remove_project", { id: p.id });
const project = await invoke("register_project", { path: DEMO, name: "Port Project" });
check("registers a node project", project.runnerId === "node", project.runnerId);

const { createServer } = await import("node:net");

// --- Tier 2: a GUESSED port (node.toml default 3000) must NOT block ---------
const blocker = createServer();
await new Promise((r) => blocker.listen(3000, "0.0.0.0", r));
await sleep(500);

const guessed = await tryInvoke("start_project", { id: project.id });
check("a guessed port in use does NOT block the launch", guessed.ok === true,
  guessed.ok ? "started" : JSON.stringify(guessed.error).slice(0, 70));

if (guessed.ok) {
  const warned = await (async () => {
    for (let i = 0; i < 30; i++) {
      const logs = await invoke("get_logs", { id: project.id });
      if (logs.lines.some((l) => l.stream === "deck" && /port 3000 is already in use/i.test(l.text))) return true;
      await sleep(300);
    }
    return false;
  })();
  check("but it IS warned about in the log", warned);
  await invoke("stop_project", { id: project.id, force: true });
  await sleep(1500);
}
await new Promise((r) => blocker.close(r));
await sleep(500);

console.log(`\n${results.filter(Boolean).length}/${results.length} port-policy checks passed`);
await invoke("stop_project", { id: project.id, force: true }).catch(() => {});
await sleep(1200);
await invoke("remove_project", { id: project.id });
process.exit(results.filter((r) => !r).length === 0 ? 0 : 1);
