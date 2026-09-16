/**
 * Shared CDP connection for the browser-side harnesses, with the one check
 * that has now wasted time twice.
 *
 * `cargo build --release` produces a binary that still points at the Vite dev
 * server, so it launches to "localhost refused to connect". This is written
 * down in ARCHITECTURE.md and in the roadmap, and it caught the author again
 * anyway -- because the symptom is not "wrong binary", it is nine unrelated
 * assertions failing at once with every measurement reading -1. Documentation
 * cannot compete with a misleading symptom; a check can.
 *
 * `connect()` therefore refuses to hand back a session pointed at the dev
 * server or sitting on a navigation error, and says which command to run.
 */

const DEV_ORIGIN = "http://localhost:1420";

export async function connect({ port = process.env.CDP_PORT ?? "9225", timeoutMs = 45000 } = {}) {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  let page = null;
  const deadline = Date.now() + timeoutMs;
  while (!page && Date.now() < deadline) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json`);
      const targets = await res.json();
      page = targets.find(
        (t) => t.type === "page" && t.webSocketDebuggerUrl && !/devtools/.test(t.url ?? ""),
      );
    } catch {
      /* not up yet */
    }
    if (!page) await sleep(1000);
  }
  if (!page) {
    console.error(`FAIL: no CDP target on port ${port}`);
    process.exit(1);
  }

  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => ((ws.onopen = resolve), (ws.onerror = reject)));

  let id = 0;
  // Per-call timeout: a stale CDP target accepts the socket but never answers,
  // which would otherwise hang the whole run with no diagnosis.
  const call = (method, params) =>
    new Promise((resolve, reject) => {
      const mid = ++id;
      const timer = setTimeout(() => {
        ws.removeEventListener("message", onmsg);
        reject(new Error(`${method} timed out after 10s`));
      }, 10000);
      const onmsg = (e) => {
        const m = JSON.parse(e.data);
        if (m.id === mid) {
          clearTimeout(timer);
          ws.removeEventListener("message", onmsg);
          resolve(m.result);
        }
      };
      ws.addEventListener("message", onmsg);
      ws.send(JSON.stringify({ id: mid, method, params }));
    });

  const ev = async (expression) => {
    const r = await call("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (r.exceptionDetails) {
      throw new Error(r.exceptionDetails.exception?.description ?? "threw");
    }
    return r.result?.value;
  };

  const where = await ev("location.href");
  if (where.startsWith(DEV_ORIGIN) || where.startsWith("chrome-error:")) {
    console.error("");
    console.error(`FAIL: the app under test is at ${where}`);
    console.error("");
    console.error("  That is the dev-server URL, which means this binary was built with");
    console.error("  `cargo build --release`. That command does not embed the frontend --");
    console.error("  only the Tauri CLI does, because it is what runs the frontend build");
    console.error("  and hands the assets to tauri-build.");
    console.error("");
    console.error("      npx tauri build --no-bundle");
    console.error("");
    console.error("  Every check below would have failed with an unrelated-looking error.");
    ws.close();
    process.exit(1);
  }

  return { ws, call, ev, page };
}

/** Pass/fail reporter shared by the harnesses so their output stays uniform. */
/**
 * Navigates to the projects list and waits for rows.
 *
 * The app LANDS on the dashboard (that is the product decision, asserted by
 * `dashboard-verify`), so every suite that measures the project table has to
 * say so rather than assuming. One helper, because eight copies of a
 * navigate-and-wait drift apart the first time the sidebar changes.
 */
export async function showProjects(ev) {
  // Returns whether rows actually appeared. Callers MUST assert it: a suite
  // that navigates, finds nothing and carries on will pass every later check
  // vacuously -- the failure mode this helper existed to prevent.
  return await ev(`(async () => {
    const go = [...document.querySelectorAll('button')]
      .find(b => b.textContent.trim().startsWith('All projects'));
    go?.click();
    // 30 x 250ms = 7.5s, deliberately under this file's own 10s per-call
    // timeout: at 20s the evaluate rejected first and the caller died with an
    // opaque "timed out" instead of the diagnostic below.
    for (let i = 0; i < 30; i++) {
      if (document.querySelectorAll('[data-project-row]').length > 0) return true;
      await new Promise(r => setTimeout(r, 250));
    }
    return false;
  })()`);
}

/**
 * Opens the log panel the way a user now does.
 *
 * The grouped list row carries ONE primary action and an overflow menu, so
 * there is no per-row "Logs for X" button any more -- suites that clicked one
 * were driving a control the design deliberately removed. The path that
 * exists: select a row, then use the selection bar's Logs button.
 *
 * Returns whether the panel actually opened, so a suite cannot proceed to
 * measure a panel that never appeared.
 */
export async function openLogPanel(ev) {
  return await ev(`(async () => {
    const settle = (ms) => new Promise(r => setTimeout(r, ms));
    if (document.querySelector('aside[aria-label^="Logs for"]')) return true;

    const row = document.querySelector('[data-project-row]');
    if (!row) return false;
    row.click();
    for (let i = 0; i < 30; i++) {
      if (document.querySelector('footer.bar-enter')) break;
      await settle(100);
    }
    const logs = document.querySelector('footer [aria-label="Logs"]');
    if (!logs) return false;
    logs.click();
    for (let i = 0; i < 40; i++) {
      if (document.querySelector('aside[aria-label^="Logs for"]')) return true;
      await settle(100);
    }
    return false;
  })()`);
}

/** Closes the log panel if it is open. Returns whether it ended up closed. */
export async function closeLogPanel(ev) {
  return await ev(`(async () => {
    const settle = (ms) => new Promise(r => setTimeout(r, ms));
    const close = document.querySelector('[aria-label="Close log panel"]');
    if (close) close.click();
    for (let i = 0; i < 40; i++) {
      if (!document.querySelector('aside[aria-label^="Logs for"]')) return true;
      await settle(100);
    }
    return false;
  })()`);
}

export function reporter() {
  let pass = 0;
  let fail = 0;
  return {
    ok(name, cond, detail = "") {
      const tail = detail ? `  - ${detail}` : "";
      if (cond) {
        pass++;
        console.log(`PASS  ${name}${tail}`);
      } else {
        fail++;
        console.log(`FAIL  ${name}${tail}`);
      }
    },
    /**
     * Prints the tally and sets the process exit code.
     *
     * Setting `process.exitCode` was missing: this used to only RETURN 0 or 1.
     * The eleven older suites do not depend on that, because each ends with
     * its own `process.exit(...)` -- verified suite by suite at `958bcaf`. The
     * two suites added in September 2026 (`shell-verify`, `regress-verify`)
     * call `finish(...)` as a bare statement instead, so both exited 0 whatever
     * they found, and `run-all.ps1` -- which decides PASS/FAIL from
     * `$LASTEXITCODE` -- printed PASS beside a suite whose own last line read
     * "4 of 27 FAILED".
     *
     * (An earlier version of this comment claimed EVERY suite had always
     * exited 0 and that the aggregate line had never been able to fail. That
     * was wrong, and a review caught it: the blind spot was two suites wide,
     * not eleven. Setting the code here is still right -- it makes the
     * reporter self-sufficient so the next suite cannot inherit the bug.)
     *
     * Assigning the code rather than calling `process.exit` lets stdout flush
     * first, which `process.exit` can truncate.
     */
    finish(label) {
      console.log("");
      console.log(
        fail === 0
          ? `${pass}/${pass} ${label} passed`
          : `${fail} of ${pass + fail} ${label} FAILED`,
      );
      process.exitCode = fail === 0 ? 0 : 1;
      return fail === 0 ? 0 : 1;
    },
  };
}
