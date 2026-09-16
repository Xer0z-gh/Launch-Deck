/**
 * Registers a fixture project, starts it, and prints its pid.
 *
 * Driver for `survives-close.ps1`. It lives in its own file because generating
 * it inside a PowerShell here-string means JS template literals, PowerShell
 * subexpressions and backtick escapes all fighting over the same characters --
 * which produced a driver that did not parse and an error that pointed at
 * PowerShell rather than at the real problem.
 *
 *   node close-fixture-driver.mjs <cdp-port> <fixture-path>
 */
const [port, fixturePath] = process.argv.slice(2);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find(
  (t) =>
    t.type === "page" &&
    /^(tauri|http):\/\/(localhost|tauri\.localhost)/.test(t.url ?? ""),
);
if (!page) throw new Error("no app CDP target");

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
let id = 0;
const call = (m, p) =>
  new Promise((res, rej) => {
    const mid = ++id;
    const t = setTimeout(() => rej(new Error("timeout " + m)), 60000);
    const h = (e) => {
      const x = JSON.parse(e.data);
      if (x.id === mid) {
        clearTimeout(t);
        ws.removeEventListener("message", h);
        res(x.result);
      }
    };
    ws.addEventListener("message", h);
    ws.send(JSON.stringify({ id: mid, method: m, params: p }));
  });
const ev = async (expr) => {
  const r = await call("Runtime.evaluate", {
    expression: expr,
    returnByValue: true,
    awaitPromise: true,
  });
  if (r.exceptionDetails)
    throw new Error(r.exceptionDetails.exception?.description ?? "threw");
  return r.result?.value;
};
await call("Runtime.enable");

const inv = (cmd, args) =>
  `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`;

// Registering a path that is already registered fails, which is correct
// behaviour and made a *rerun* of this suite look like a product bug. Reuse the
// existing registration instead of treating the second run as a failure.
let projectId = await ev(
  `${inv("register_project", { path: fixturePath, name: "ZZ-Close-Fixture" })}
     .then(p => p.id).catch(() => null)`,
);
if (!projectId) {
  projectId = await ev(
    `${inv("list_projects", {})}
       .then(ps => (ps.find(p => String(p.root).toLowerCase() === ${JSON.stringify(
         fixturePath.replace(/\+$/, "").toLowerCase(),
       )}) ?? {}).id ?? null)
       .catch(() => null)`,
  );
}
if (!projectId) {
  console.error("could not register or find the fixture project");
  console.log("PID=");
  ws.close();
  process.exit(1);
}

// `--remove` is the cleanup pass, run at the end of `survives-close.ps1`.
if (process.argv.includes("--remove")) {
  await ev(`${inv("remove_project", { id: projectId })}.catch(() => null)`);
  console.log("PID=");
  ws.close();
  process.exit(0);
}

await ev(`${inv("start_project", { id: projectId })}.catch(() => null)`);
await sleep(4000);
const metrics = await ev(inv("project_metrics", { id: projectId }));
console.log("PID=" + ((metrics ?? []).slice(-1)[0]?.pid ?? ""));
ws.close();
