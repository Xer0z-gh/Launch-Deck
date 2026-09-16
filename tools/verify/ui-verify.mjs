/**
 * Verifies the release UI: Photoshop-neutral theme, LaunchBox library flow,
 * and real project icon previews. DOM/CSSOM level — no dev hooks, since the
 * release build exposes none.
 */
const PORT = process.env.CDP_PORT ?? "9225";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let page = null;
for (let i = 0; i < 45 && !page; i++) {
  try {
    const res = await fetch(`http://127.0.0.1:${PORT}/json`);
    const targets = await res.json();
    page = targets.find((t) => t.type === "page" && t.webSocketDebuggerUrl && !/devtools/.test(t.url ?? ""));
  } catch { /* not up */ }
  if (!page) await sleep(1000);
}
if (!page) { console.error("FAIL: no CDP target"); process.exit(1); }

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r, j) => ((ws.onopen = r), (ws.onerror = j)));
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
  const r = await call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
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

// The destination persists between runs, so a harness that assumes the project
// list is showing fails with "no table" after any run that ended on another
// view. Set the starting point explicitly rather than inheriting it.
await ev(`[...document.querySelectorAll("nav button")].find(x=>x.textContent.includes("All projects"))?.click()`);
await sleep(900);

const pageUrl = await ev("location.origin");
console.log(`${pageUrl === "http://tauri.localhost" ? "PASS" : "FAIL"}  assets served from the embedded protocol, not a dev server  (${pageUrl})`);
if (pageUrl !== "http://tauri.localhost") {
  console.error("This binary points at a dev server; it was not built via the Tauri CLI.");
  process.exit(1);
}

// Wait for projects to load.
for (let i = 0; i < 40; i++) {
  const t = await ev("document.body.textContent");
  if (/library/i.test(t) && (t.includes("Vesper") || t.includes("No projects"))) break;
  await sleep(1000);
}

const results = [];
const check = (name, ok, detail = "") => {
  results.push(ok);
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  (${detail})` : ""}`);
};

// ---- Theme: Photoshop neutrals, zero blue cast -----------------------------
const tokens = await ev(`(() => {
  const s = getComputedStyle(document.documentElement);
  return ["field","panel","raised","ink","accent","rule"].reduce((acc,k) => {
    acc[k] = s.getPropertyValue("--color-" + k).trim(); return acc;
  }, {});
})()`);
const rgb = (value) => {
  const v = value.trim();
  // Three-digit hex too: the browser serialises `#000000` back as `#000`,
  // which made a correct token look like a failed one.
  const short = v.match(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/i);
  if (short) return short.slice(1).map((c) => parseInt(c + c, 16));
  // Six OR eight digits: a token authored as `rgb(84 84 88 / 0.6)` comes back
  // as `#54545899`, and dropping the alpha is correct here because every
  // comparison below is about the colour, not its opacity.
  const long = v.match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})(?:[0-9a-f]{2})?$/i);
  if (long) return long.slice(1, 4).map((c) => parseInt(c, 16));
  const fn = v.match(/^rgba?\(([^)]+)\)$/i);
  if (fn) {
    const parts = fn[1].split(/[\s,/]+/).filter(Boolean).map(Number);
    if (parts.length >= 3 && parts.slice(0, 3).every((n) => Number.isFinite(n))) {
      return parts.slice(0, 3);
    }
  }
  return null;
};
const lum = (hex) => { const c = rgb(hex); return c ? c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114 : NaN; };
/** How far from grey a colour is, in raw 0-255 channel spread. */
const cast = (hex) => { const c = rgb(hex); return c ? Math.max(...c) - Math.min(...c) : NaN; };

// The palette is allowed to be retuned; the ladder's RULES are not. Pinning
// exact hexes froze the design and turned every deliberate change into six
// simultaneous failures that said nothing about correctness.
check("surface ladder ascends: field < panel < raised",
  lum(tokens.field) < lum(tokens.panel) && lum(tokens.panel) < lum(tokens.raised),
  `${tokens.field} ${tokens.panel} ${tokens.raised}`);
// Inverted deliberately on 2026-07-31. The app used to draw lines with a
// near-black seam colour; on a #121212 field that reads as a crack rather than
// a divider, and it meant menu separators never matched row dividers. There is
// now ONE line colour and it is LIGHTER than the surfaces it is drawn on.
check("separators are LIGHTER than the surfaces they divide",
  lum(tokens.rule) > lum(tokens.field) && lum(tokens.rule) > lum(tokens.panel),
  `rule ${tokens.rule} vs field ${tokens.field} / panel ${tokens.panel}`);
// A deliberate hint of temperature is wanted; an actual colour cast is not.
check("greys carry at most a hint of cast, never a hue",
  [tokens.field, tokens.panel, tokens.raised].every((h) => cast(h) <= 8),
  [tokens.field, tokens.panel, tokens.raised].map((h) => `${h}:${cast(h)}`).join(" "));
// REVERSED 2026-08-31, on Tanner's instruction ("really nail that apple
// styling fully switch the ui to this" + the iOS/iPadOS 27 kit).
//
// This used to assert the field was NEVER black: the charcoal was chosen so
// near-black under near-white would not sit at ~17:1, which is past the point
// where more contrast helps and is the main cause of strain over an hour.
// That reasoning was sound and is why the rule existed -- it is not being
// dropped by accident.
//
// iOS dark mode uses true black for the grouped background, and every
// elevated grey in Apple's ladder is calibrated against it; keeping a
// charcoal field would have left every card reading a step too light. So the
// constraint changes shape rather than disappearing: the FIELD may now be
// black, but the ladder above it must still ascend, and the LABEL must not
// be pure white -- which is what actually caused the strain, and which iOS
// avoids too (its label steps are opacities of a tinted white, not #fff).
check("field is the darkest surface, and the ladder still ascends",
  lum(tokens.field) <= lum(tokens.panel) && lum(tokens.panel) < lum(tokens.raised),
  `field ${lum(tokens.field).toFixed(1)} panel ${lum(tokens.panel).toFixed(1)} raised ${lum(tokens.raised).toFixed(1)}`);
check("body text is never pure white on the dark theme",
  lum(tokens.ink) < 250, `ink ${tokens.ink} luminance ${lum(tokens.ink).toFixed(1)}`);
// Not a pinned hex -- the accent is allowed to be retuned. What must hold is
// that it stays VIVID (a washed-out accent cannot mean "live" on a near-black
// field) and stays far from the warn hue, so "live" and "wrong" are never
// separated by lightness alone.
const chroma = (hex) => { const c = rgb(hex); return c ? Math.max(...c) - Math.min(...c) : 0; };
check("accent is vivid enough to mean 'live'", chroma(tokens.accent) >= 60,
  `${tokens.accent} chroma ${chroma(tokens.accent)}`);

// Return raw strings and parse HERE. A backslash inside a JS template literal
// is an escape sequence, so a `\d` written in the evaluated string arrives at
// the page as a plain `d` and the regex silently matches nothing.
const bodyBg = await ev(`(() => ({
  body: getComputedStyle(document.body).backgroundColor,
  field: getComputedStyle(document.documentElement).getPropertyValue("--color-field").trim(),
}))()`);
const bodyChannels = (bodyBg.body.match(/[0-9]+/g) ?? []).map(Number);
const fieldRgb = rgb(bodyBg.field);
check("body actually paints the field token",
  fieldRgb !== null && bodyChannels.length >= 3
    && bodyChannels.slice(0, 3).every((v, i) => v === fieldRgb[i]),
  `body ${bodyBg.body} vs ${bodyBg.field}`);

// ---- LaunchBox flow: sidebar library + tile grid + select-to-detail --------
const sidebar = await ev(`(() => {
  const nav = document.querySelector('nav[aria-label="Project collections"]');
  if (!nav) return null;
  return { text: nav.textContent, buttons: nav.querySelectorAll("button").length };
})()`);
check("library sidebar present", sidebar !== null);
check("has fixed collections", Boolean(sidebar &&
  ["All projects","Favourites","Running","Archived"].every((l) => sidebar.text.includes(l))));
check("groups by language", Boolean(sidebar && /languages/i.test(sidebar.text)),
  `${sidebar?.buttons ?? 0} collection buttons`);

// ---- Main view: Docker-style rows, with controls that cannot be clipped ----

// Poll: React commits the table a frame or two after the query resolves.
let rows = 0;
for (let i = 0; i < 30 && rows === 0; i++) {
  rows = await ev(`document.querySelectorAll("[data-project-row]").length`);
  if (rows === 0) await sleep(300);
}
check("Docker-style row list is the default view", rows > 5, `${rows} rows`);

// THE reported bug: the actions column was pushed past the right edge behind a
// horizontal scrollbar. Assert both causes are gone.
const layout = await ev(`(() => {
  const wrap = document.querySelector("ul:has([data-project-row])")?.parentElement;
  if (!wrap) return null;
  const row = document.querySelector("[data-project-row]");
  // The row's controls are its own buttons; there is no actions COLUMN in a
  // grouped list, and the chip is a control too -- so read the row directly.
  const buttons = row ? [...row.querySelectorAll("button")] : [];
  const wrapRect = wrap.getBoundingClientRect();
  return {
    overflowsHorizontally: wrap.scrollWidth > wrap.clientWidth + 1,
    buttonCount: buttons.length,
    allOpaque: buttons.every((b) => Number(getComputedStyle(b).opacity) === 1),
    // Only VISIBLE buttons can be "inside": below a 480px container the row
    // deliberately hides Restart and Logs into the overflow menu, so they have
    // zero width by design. Requiring width > 0 of every button asserts the old
    // always-show-everything behaviour, not containment.
    allInsideViewport: buttons
      .filter((b) => b.getBoundingClientRect().width > 0)
      .every((b) => {
        const r = b.getBoundingClientRect();
        return r.right <= wrapRect.right + 1 && r.left >= wrapRect.left - 1;
      }),
    rightmostOverhang: Math.max(0, ...buttons.map((b) =>
      b.getBoundingClientRect().right - wrapRect.right)),
  };
})()`);
// The grouped list has no columns to push out; what must hold is that the
// row's controls stay inside the card at every width, checked just below.
check("no horizontal scrolling on the list", layout?.overflowsHorizontally === false);
check("every row exposes its action controls", (layout?.buttonCount ?? 0) >= 2,
  `${layout?.buttonCount ?? 0} buttons per row`);
check("action controls are visible without hovering", layout?.allOpaque === true);
check("action controls sit fully inside the list", layout?.allInsideViewport === true,
  `overhang ${layout?.rightmostOverhang ?? "?"}px`);

// Narrow the window: actions must survive when the row sheds detail.
await call("Emulation.setDeviceMetricsOverride",
  { width: 900, height: 700, deviceScaleFactor: 1, mobile: false });
await sleep(500);
const narrow = await ev(`(() => {
  const wrap = document.querySelector("ul:has([data-project-row])")?.parentElement;
  const row = document.querySelector("[data-project-row]");
  if (!wrap || !row) return null;
  const buttons = [...row.children[row.children.length - 1].querySelectorAll("button")];
  const wrapRect = wrap.getBoundingClientRect();
  const shown = buttons.filter((b) => b.getBoundingClientRect().width > 0);
  const labels = shown.map((b) => b.getAttribute("aria-label") ?? "");
  return {
    overflows: wrap.scrollWidth > wrap.clientWidth + 1,
    visible: shown.length,
    inside: shown.every((b) => b.getBoundingClientRect().right <= wrapRect.right + 1),
    hasPrimary: labels.some((l) => l.startsWith("Run ") || l.startsWith("Stop ")),
    hasOverflow: labels.some((l) => l.startsWith("More actions")),
  };
})()`);
check("at 900px wide: still no horizontal scrolling", narrow?.overflows === false);
// Deliberately not a count. Below a ~480px container the row sheds Restart and
// Logs into the overflow menu (both are menu entries, so nothing becomes
// unreachable) so the name column keeps a readable width. The invariant that
// must hold at every width is: primary action present, menu present, nothing
// clipped. A raw ">= 4" would fail here for a designed behaviour while still
// passing if a button were pushed outside the container.
check("at 900px wide: primary action and overflow menu visible, nothing clipped",
  narrow?.inside === true && narrow?.hasPrimary === true && narrow?.hasOverflow === true,
  `${narrow?.visible} visible, primary=${narrow?.hasPrimary}, overflow=${narrow?.hasOverflow}`);
await call("Emulation.clearDeviceMetricsOverride");
await sleep(300);

// Selecting a row must raise the detail bar with a prominent Run.
const clicked = await ev(`(() => {
  const r = document.querySelector("[data-project-row]");
  if (!r) return false;
  r.click();
  return true;
})()`);
check("a row is clickable", clicked);
await sleep(400);
const detail = await ev(`(() => {
  const bar = document.querySelector('footer[aria-label^="Selected:"]');
  return bar ? { text: bar.textContent, hasRun: /Run|Stop/.test(bar.textContent) } : null;
})()`);
check("clicking a row opens the detail bar", detail !== null, detail?.text?.slice(0, 40) ?? "");
check("detail bar offers a prominent Run/Stop", Boolean(detail?.hasRun));

// ---- Icon previews: real artwork, not just glyph fallbacks -----------------
const icons = await ev(`(() => {
  const imgs = [...document.querySelectorAll('img[src^="data:image"]')];
  return {
    count: imgs.length,
    loaded: imgs.filter((i) => i.complete && i.naturalWidth > 0).length,
    kinds: [...new Set(imgs.map((i) => i.src.slice(5, i.src.indexOf(";"))))],
    maxNatural: Math.max(0, ...imgs.map((i) => i.naturalWidth)),
  };
})()`);
check("real project icons rendered as data URLs", icons.count > 0, `${icons.count} icons`);
check("every icon decoded successfully", icons.count > 0 && icons.loaded === icons.count,
  `${icons.loaded}/${icons.count} loaded`);
check("icons are genuine image payloads", icons.kinds.length > 0, icons.kinds.join(", "));
check("icon source resolution is usable", icons.maxNatural >= 32, `${icons.maxNatural}px largest`);

// ---- Release hygiene -------------------------------------------------------
check("no dev hook in release", !(await ev(`typeof window.__deck !== "undefined"`)));
check("no global Tauri API in release", !(await ev(`typeof window.__TAURI__ !== "undefined"`)));
const errs = await ev(`(window.__errors__ || []).length`).catch(() => 0);
check("no page errors recorded", !errs);

const failed = results.filter((r) => !r).length;
console.log(`\n${results.length - failed}/${results.length} UI checks passed`);
process.exit(failed === 0 ? 0 : 1);
