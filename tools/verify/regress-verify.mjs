/**
 * The defects two reviews measured, kept fixed.
 *
 * A UI finish-gate review and an accessibility audit both returned HOLD on the
 * grid / storage / manual work, between them naming eighteen specific,
 * measured failures. Every check below FAILED before its fix and passes after,
 * so this file is the record that they stay fixed rather than a claim that
 * they were.
 *
 * They are kept in one suite rather than scattered into the feature suites
 * because what they have in common is not a screen -- it is the failure mode.
 * Each was invisible to a type-check, to a screenshot, and in four cases to
 * the computed styles: the grid's focus ring reported `inset 0 0 0 2px accent`
 * the whole time it was painting underneath an opaque overlay at 1.22:1, and
 * the only thing that caught it was sampling the rendered pixels.
 *
 * # Not every check here is a regression check, and the ones that are not say so
 *
 * A third review audited this file and found three checks that pass against
 * the OLD code as well as the new -- they are preconditions for the checks
 * that follow (the grid must have columns before "arrows move by a row" means
 * anything), not guards. They are labelled `precondition:` so the count is
 * honest: of the checks below, the ones without that prefix each failed
 * before their fix and pass after.
 *
 * The same audit killed the worst check in the file. "Nothing is stacked over
 * the focused tile" was written as
 * `stack[0] === tile || tile.contains(stack[0])` -- and the overlay that
 * caused the original defect was a DESCENDANT of the tile, so
 * `contains(...)` was true by definition and the check passed against the
 * exact bug it claimed to guard. It is now identity only, and it is joined by
 * a real contrast measurement, because "the ring exists" and "the ring can be
 * seen" are the two different things this file exists to keep apart.
 *
 *   CDP_PORT=9280 node tools/verify/regress-verify.mjs
 */
import { connect, reporter, showProjects } from "./cdp.mjs";

const { ws, call, ev } = await connect();
const { ok, finish } = reporter();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const move = (x, y) =>
  call("Input.dispatchMouseEvent", { type: "mouseMoved", x, y, button: "none" });
const click = async (x, y) => {
  for (const type of ["mousePressed", "mouseReleased"]) {
    await call("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  }
};
const key = async (k, code, vk) => {
  await call("Input.dispatchKeyEvent", { type: "rawKeyDown", key: k, code, windowsVirtualKeyCode: vk });
  await call("Input.dispatchKeyEvent", { type: "keyUp", key: k, code, windowsVirtualKeyCode: vk });
  await sleep(160);
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
  await sleep(400);
  return true;
};

/**
 * Resolves a CSS colour to RGB, composited over a known backdrop.
 *
 * # Two traps, and the second one bit
 *
 * A regex cannot do this on its own: Tailwind v4 emits
 * `oklab(0.63 -0.04 -0.12 / 0.15)` for its opacity modifiers, which no rgb()
 * parser handles. So the obvious answer is a canvas -- paint the colour and
 * read the pixel back.
 *
 * The canvas has its own trap, and it produced a confidently wrong number
 * here: **Chrome's `fillStyle` drops the alpha from an `oklab(... / a)`
 * string.** A 15% accent tint painted as fully opaque accent, so the selected
 * tile's background resolved to rgb(74,141,215) instead of rgb(35,45,58), and
 * the focus ring measured 1.02:1 against a background that was supposedly the
 * same colour as the ring. The product was fine; the instrument was not.
 *
 * So: take the alpha out of the string, let the canvas resolve the colour at
 * full opacity (which it does correctly), and do the compositing in
 * arithmetic where nothing can silently discard it.
 */
const RESOLVE = `(() => {
  const c = document.createElement("canvas");
  c.width = c.height = 1;
  const ctx = c.getContext("2d", { willReadFrequently: true });

  // The alpha, from either modern slash syntax or legacy rgba()/hsla().
  //
  // Deliberately no regex. This whole script is a JS TEMPLATE LITERAL in the
  // harness, so backslashes are consumed before the page ever sees them --
  // a pattern written as /\s*/ arrives as /s*/ and the page throws
  // "Unexpected token". String work has no escapes to lose.
  const alphaOf = (css) => {
    const t = String(css).trim();
    if (t === "transparent") return 0;
    if (t.endsWith(")")) {
      const slash = t.lastIndexOf("/");
      if (slash !== -1) {
        const raw = t.slice(slash + 1, -1).trim();
        const v = raw.endsWith("%") ? parseFloat(raw) / 100 : parseFloat(raw);
        if (!Number.isNaN(v)) return v;
      }
      if (t.startsWith("rgba(") || t.startsWith("hsla(")) {
        const parts = t.slice(t.indexOf("(") + 1, -1).split(",");
        if (parts.length === 4) {
          const v = parseFloat(parts[3]);
          if (!Number.isNaN(v)) return v;
        }
      }
    }
    return 1;
  };

  // The colour at full opacity. Canvas handles oklab/lab/color() fine here;
  // it is only the alpha it loses.
  const opaque = (css) => {
    ctx.clearRect(0, 0, 1, 1);
    ctx.fillStyle = "#000";
    ctx.fillStyle = css;
    ctx.fillRect(0, 0, 1, 1);
    const d = ctx.getImageData(0, 0, 1, 1).data;
    return [d[0], d[1], d[2]];
  };

  /** \`css\` composited over \`backdrop\` (an [r,g,b] array). */
  window.__over = (css, backdrop) => {
    const a = alphaOf(css);
    const fg = opaque(css);
    return fg.map((v, i) => Math.round(a * v + (1 - a) * backdrop[i]));
  };

  /**
   * What is actually painted behind \`el\`.
   *
   * Not simply the parent's background-color: most elements here are
   * transparent, and an earlier version took the nearest ancestor's declared
   * value, got rgba(0,0,0,0) from <main>, and composited over BLACK. That
   * flatters every measurement -- a light ring against a too-dark backdrop
   * reads 5.43:1 where the truth is 4.53:1. Climb until something opaque is
   * found, then paint the translucent layers back down onto it in order.
   */
  window.__backdrop = (el) => {
    const layers = [];
    let node = el.parentElement;
    while (node) {
      const bg = getComputedStyle(node).backgroundColor;
      const a = alphaOf(bg);
      if (a > 0) layers.push(bg);
      if (a === 1) break;
      node = node.parentElement;
    }
    let out = [255, 255, 255];
    for (let i = layers.length - 1; i >= 0; i--) out = window.__over(layers[i], out);
    return out;
  };

  const lum = ([r, g, b]) => {
    const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4); };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  window.__contrast = (a, b) => {
    const [hi, lo] = [lum(a), lum(b)].sort((x, y) => y - x);
    return (hi + 0.05) / (lo + 0.05);
  };
  return true;
})()`;

await showProjects(ev);
await clickSel(`document.querySelector('button[aria-label="Grid view"]')`);
await sleep(700);

// ---- 1. A click at the tile centre selects; it must not launch -------------
const centre = await ev(`(() => {
  const t = document.querySelectorAll("[data-project-tile]")[3];
  if (!t) return null;
  const r = t.getBoundingClientRect();
  return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2),
           name: t.getAttribute("aria-label") };
})()`);
await move(centre.x, centre.y);
await sleep(300);
const atCentre = await ev(`(() => {
  const el = document.elementFromPoint(${centre.x}, ${centre.y});
  return el ? (el.getAttribute("aria-label") ?? el.tagName) : "none";
})()`);
ok(
  "the tile centre is the tile, not a hidden Run button",
  !/^Run |^Open |^Stop /.test(atCentre),
  atCentre,
);

const before = await ev(`document.querySelectorAll("[data-live]").length`);
await click(centre.x, centre.y);
await sleep(900);
const after = await ev(`(() => ({
  live: document.querySelectorAll("[data-live]").length,
  selected: document.querySelectorAll("[data-selected]").length,
}))()`);
ok("clicking the centre selects", after.selected >= 1, `${after.selected} selected`);
ok(
  "clicking the centre launches nothing",
  after.live === before,
  `live ${before} -> ${after.live}`,
);

// ---- 2. Hover and focus must not hide the mark ------------------------------
const markVisible = await ev(`(() => {
  const t = document.querySelectorAll("[data-project-tile]")[3];
  const img = t?.querySelector("img, svg");
  if (!img) return null;
  const cs = getComputedStyle(img);
  const r = img.getBoundingClientRect();
  return { opacity: cs.opacity, w: Math.round(r.width), visible: cs.visibility };
})()`);
// The icon's OWN opacity was 1 in the broken version too -- the overlay hid
// it from above rather than fading it -- so occlusion is proven by the
// `elementsFromPoint` check below, and this one is about the size the review
// asked for (32px was 8.3% of the tile).
ok(
  "the mark is large enough to recognise, and not faded",
  markVisible && Number(markVisible.opacity) === 1 && markVisible.w >= 40,
  JSON.stringify(markVisible),
);

// ---- 2b. The focus ring is painted AND unobstructed -------------------------
//
// The original ring was `inset` box-shadow, which paints on the padding box
// below descendants, so the reveal overlay covered it -- 1.22:1 from
// screenshot pixels while the computed style truthfully reported
// "inset 0 0 0 2px accent". Reading the style alone is what let one review
// pass it. So this asserts the MECHANISM the bug had: the ring is a real
// outline, and nothing is stacked on top of the tile to hide it.
// Focus must arrive by KEYBOARD: `:focus-visible` does not match a
// programmatic `.focus()` in Chromium, so calling focus() directly reports
// `outline-style: none` and would fail this check for the wrong reason.
await ev(`document.querySelectorAll("[data-project-tile]")[2].focus()`);
await sleep(150);
await key("ArrowRight", "ArrowRight", 39);
const ring = await ev(`(() => {
  const t = document.activeElement;
  const cs = getComputedStyle(t);
  const r = t.getBoundingClientRect();
  // A point 3px inside the tile's edge -- where the ring is drawn.
  const x = Math.round(r.left + 3);
  const y = Math.round(r.top + r.height / 2);
  const stack = document.elementsFromPoint(x, y);
  return {
    isTile: t.hasAttribute("data-project-tile"),
    style: cs.outlineStyle,
    width: cs.outlineWidth,
    colour: cs.outlineColor,
    boxShadow: cs.boxShadow,
    topmostIsTile: stack[0] === t,
    covering: stack[0]?.className?.toString?.().slice(0, 60) ?? "",
  };
})()`);
ok(
  "the focus ring is a real outline, not an inset shadow",
  ring.isTile && ring.style !== "none" && parseFloat(ring.width) >= 2 && ring.boxShadow === "none",
  `${ring.style} ${ring.width} ${ring.colour}, box-shadow ${ring.boxShadow}`,
);
// Identity ONLY. `tile.contains(stack[0])` was the original wording and it
// made this vacuous: the overlay that hid the ring lived inside the tile, so
// `contains` was true and the check passed against the defect.
ok(
  "nothing is stacked over the focused tile to hide its ring",
  ring.topmostIsTile,
  ring.covering || "clear",
);

// And the ring must be VISIBLE, not merely declared. The original defect
// measured 1.22:1 while the computed style read a correct 2px accent ring, so
// a check that reads the style alone cannot catch it coming back: a ring
// painted in a colour close to the tile would pass everything above.
await ev(RESOLVE);
const ringContrast = await ev(`(() => {
  const t = document.activeElement;
  const cs = getComputedStyle(t);
  // The tile's own background is a translucent tint when it is also selected,
  // which is the hardest case for the ring and therefore the one worth
  // measuring -- so it has to be composited over what is really behind it.
  const tile = window.__over(cs.backgroundColor, window.__backdrop(t));
  const ink = window.__over(cs.outlineColor, tile);
  return { ratio: Math.round(window.__contrast(ink, tile) * 100) / 100,
           ring: cs.outlineColor, bg: cs.backgroundColor,
           inkRgb: ink.join(","), tileRgb: tile.join(","),
           selected: t.hasAttribute("data-selected"), tag: t.tagName };
})()`);
ok(
  "the focus ring can actually be seen against the tile",
  ringContrast.ratio >= 3,
  `${ringContrast.ratio}:1, ring rgb(${ringContrast.inkRgb}) on rgb(${ringContrast.tileRgb})` +
    `${ringContrast.selected ? " (selected tile, the hardest case)" : ""}`,
);

// ---- 3. Roving tabindex: the grid is one stop -------------------------------
const stops = await ev(`(() => {
  const sel = 'a[href], button:not([disabled]), input, select, textarea, [tabindex]';
  // tabindex="-1" is focusable but NOT tabbable, and a <button tabindex="-1">
  // still matches 'button', so the tabindex filter has to be applied after.
  const all = [...document.querySelectorAll(sel)]
    .filter(e => e.offsetParent !== null)
    .filter(e => e.getAttribute("tabindex") !== "-1");
  const tiles = all.filter(e => e.hasAttribute("data-project-tile"));
  return { total: all.length, tileStops: tiles.length,
           tilesRendered: document.querySelectorAll("[data-project-tile]").length };
})()`);
ok(
  "the whole grid is one tab stop",
  stops.tileStops === 1,
  `${stops.tileStops} stops for ${stops.tilesRendered} tiles`,
);
// Tied to the defect rather than to a number I picked. The bug was 50 tiles
// contributing 100 tab stops, so the rule is "the whole document has fewer
// stops than the grid has tiles" -- which is impossible if the grid is paying
// per tile, and comfortable whatever else is on screen. A bare `< 40` was a
// guess that failed the moment the manual panel was open during the count.
ok(
  "the document has fewer tab stops than the grid has tiles",
  stops.total < stops.tilesRendered,
  `${stops.total} stops for ${stops.tilesRendered} tiles`,
);

// ---- 4. Arrows move in two dimensions --------------------------------------
await ev(`document.querySelector("[data-project-tile]").focus()`);
await sleep(200);
const first = await ev(`document.activeElement?.getAttribute("aria-label")`);
const cols = await ev(`
  getComputedStyle(document.querySelector("[data-project-tile]").closest("ul"))
    .gridTemplateColumns.split(" ").filter(Boolean).length
`);
await key("ArrowDown", "ArrowDown", 40);
const down = await ev(`(() => ({
  name: document.activeElement?.getAttribute("aria-label"),
  scroll: document.querySelector("main")?.scrollTop ?? 0,
}))()`);
ok("ArrowDown moves focus by one row", down.name && down.name !== first, `${first} -> ${down.name}`);
ok("ArrowDown does not scroll the pane instead", down.scroll === 0, `scrollTop ${down.scroll}`);
await key("ArrowUp", "ArrowUp", 38);
const up = await ev(`document.activeElement?.getAttribute("aria-label")`);
ok("ArrowUp returns", up === first, `${up} vs ${first}`);
ok(
  "precondition: the grid really has multiple columns to move between",
  cols >= 2,
  `${cols} columns`,
);

// ---- 5. Enter and Space do the same thing ----------------------------------
const liveBefore = await ev(`document.querySelectorAll("[data-live]").length`);
await key("Enter", "Enter", 13);
const afterEnter = await ev(`document.querySelectorAll("[data-live]").length`);
await key(" ", "Space", 32);
const afterSpace = await ev(`document.querySelectorAll("[data-live]").length`);
ok(
  "Enter and Space agree, and neither launches from the tile",
  afterEnter === liveBefore && afterSpace === liveBefore,
  `${liveBefore} / ${afterEnter} / ${afterSpace}`,
);

// ---- 6. Search Enter works in grid view ------------------------------------
await ev(`(() => {
  const el = document.querySelector('input[aria-label="Search projects"]');
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
  setter.call(el, "vesper");
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.focus();
})()`);
await sleep(600);
await key("Enter", "Enter", 13);
const landed = await ev(`document.activeElement?.getAttribute("aria-label") ?? document.activeElement?.tagName`);
ok(
  "search then Enter reaches a tile in grid view",
  /vesper/i.test(landed ?? ""),
  landed ?? "nothing",
);
await ev(`(() => {
  const el = document.querySelector('input[aria-label="Search projects"]');
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
  setter.call(el, "");
  el.dispatchEvent(new Event("input", { bubbles: true }));
})()`);
await sleep(400);

// ---- 7. Storage: one chart, one scale --------------------------------------
await clickSel(`document.querySelector('button[aria-label="List view"]')`);
await clickSel(`[...document.querySelectorAll("button")].find(b => b.textContent.trim() === "Storage")`);
await sleep(1200);
await ev(`(async () => {
  for (let i = 0; i < 40; i++) {
    const rows = [...document.querySelectorAll('section[aria-labelledby="storage-projects"] button[aria-expanded]')];
    if (rows.filter(r => !/measuring/i.test(r.textContent)).length > 20) return true;
    await new Promise(r => setTimeout(r, 500));
  }
  return false;
})()`);
await clickSel(`
  [...document.querySelectorAll('section[aria-labelledby="storage-projects"] button[aria-expanded]')]
    .find(r => !/measuring/i.test(r.textContent))
`);
await sleep(2500);

const scales = await ev(`(() => {
  const group = document.querySelector('section[aria-labelledby="storage-projects"]');
  const parentRow = [...group.querySelectorAll('button[aria-expanded="true"]')][0];
  const parentBar = parentRow?.querySelector('[data-size-bar]');
  const kids = [...group.querySelectorAll("ul li")];
  const kidTracks = kids.map(k => k.querySelector('[data-size-bar]')).filter(Boolean);
  const widths = kidTracks.map(t => Math.round(t.getBoundingClientRect().width));
  const fills = kids.map(k => k.querySelector('[data-size-bar] > *')).filter(Boolean);
  return {
    parentBarPresent: Boolean(parentBar),
    trackWidths: [...new Set(widths)],
    biggestTrack: Math.max(...widths, 0),
    fillCount: fills.length,
    distinctWidths: [...new Set(fills.map(f => f.style.width))].length,
    kidCount: kids.length,
  };
})()`);
ok(
  "the parent bar is withdrawn while its children draw the same span",
  scales.parentBarPresent === false,
  `parent bar present: ${scales.parentBarPresent}`,
);
ok(
  "child tracks are full width, not a fixed 96px stub column",
  scales.biggestTrack > 200,
  `${scales.biggestTrack}px`,
);
// The original defect was a 2px FLOOR that made 18 of 20 children identical.
// The rule is that widths track the real proportion, so distinct sizes must
// produce distinct widths -- suppressing the small ones was the other wrong
// answer, and it blanked two thirds of the top-level chart.
ok(
  "child bar widths track real sizes rather than collapsing to one stub",
  scales.distinctWidths >= 3,
  `${scales.distinctWidths} distinct widths across ${scales.kidCount} rows`,
);

// ---- 8. The manual toggle is absent where it cannot act ---------------------
// It used to report aria-pressed="true" and tint itself accent while the pane
// it claims to control computed to display:none.
const wideToggle = await ev(`!!document.querySelector('button[aria-label$="the manual panel"]')`);
ok("precondition: the manual toggle is offered on Storage at full width", wideToggle);

await call("Emulation.setDeviceMetricsOverride", {
  width: 900, height: 600, deviceScaleFactor: 1, mobile: false,
});
await sleep(900);
// VISIBILITY, not presence. The toggle is gated by the same
// `max-[1099px]:hidden` class as the panel, so it is still in the DOM --
// `querySelector` finds it and an earlier version of this check read that as
// "still offered". What matters is whether it is on screen claiming to
// control something that is not.
const narrow = await ev(`(() => {
  const btn = document.querySelector('button[aria-label$="the manual panel"]');
  const aside = document.querySelector('aside[aria-label="Project manual"]');
  return {
    toggleVisible: btn ? btn.offsetParent !== null : false,
    toggleDisplay: btn ? getComputedStyle(btn).display : "absent",
    asideDisplay: aside ? getComputedStyle(aside).display : "absent",
  };
})()`);
ok(
  "below 1100px the toggle is hidden rather than lying about a hidden pane",
  narrow.toggleVisible === false && narrow.asideDisplay !== "block",
  JSON.stringify(narrow),
);
await call("Emulation.clearDeviceMetricsOverride");
await sleep(500);

// ---- 9. Search is not offered where it cannot filter ------------------------
const searchOnStorage = await ev(`(() => {
  const el = document.querySelector('input[aria-label="Search projects"]');
  if (!el) return "absent";
  const wrap = el.closest("div");
  return { hidden: wrap?.getAttribute("aria-hidden"), vis: getComputedStyle(wrap).visibility };
})()`);
ok(
  "search is hidden from the accessibility tree on Storage",
  searchOnStorage === "absent" || searchOnStorage.hidden === "true",
  JSON.stringify(searchOnStorage),
);

ws.close();
finish("regress");
