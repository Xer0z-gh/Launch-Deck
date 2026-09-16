# LAUNCH DECK — DESIGN

The rules this interface is built from, and the reasoning behind each one.
Written to be *checkable*: every rule below is either enforced by a token,
verified by a harness, or stated as a decision someone can argue with.

A rule with no reason is a preference. Preferences drift. Reasons hold.

======================================================================
0 — WHAT THIS PRODUCT IS
======================================================================

An expert tool, used briefly and often, by one person who already knows
what every row means.

    User          Tanner. Builds these projects. Knows their names,
                  their stacks, and what "crashed" implies.
    Frequency     Many times a day, for seconds at a time.
    Dominant act  Find one project in a list and press Run.
    Session       Short bursts — but the window stays open all day,
                  visible in peripheral vision.

Every decision below follows from those four lines. Three consequences
worth naming up front, because they are what make this app not a website:

• **Density is a feature.** Scanning 40 projects must not require
  scrolling. Whitespace that would be generous on a marketing page is
  wasted space here.
• **No onboarding, no explanation, no encouragement.** The user knows
  more about these projects than the app does.
• **Idle appearance matters more than active appearance.** This window
  sits in the corner of a monitor for hours. It must be quiet when
  nothing is happening, and it must be impossible to misread at a
  glance. Those are different requirements from "looks good in a
  screenshot", and where they conflict, screenshots lose.

======================================================================
1 — SURFACES: THE PHOTOSHOP LADDER
======================================================================

Dark grey, and genuinely NEUTRAL: R = G = B on every step.

    --color-field    #121212   the room          lum 18
    --color-panel    #1a1a1a   things in it      lum 26
    --color-raised   #242424   under the cursor  lum 36
    --color-overlay  #2e2e2e   menus, floating   lum 46
    --color-rule     #2f2f2f   every line, LIGHTER than what it divides

**One line colour for the whole app** (settled 2026-07-31). There used
to be two: a near-black `edge` for "grooves between panels" and a
lighter `rule` for dividers inside them. The distinction was real in
theory and invisible in practice -- on a #121212 field a #0a0a0a seam
reads as a crack, not a groove, and two line colours meant the menu
separators, the dialog rules and the row dividers never quite matched.
Tanner asked for all of them to be the light one.

The simpler rule is also easier to keep: **if it is a line, it is
`rule`.** `edge-strong` survives only for control borders -- the outline
button and the scrollbar thumb -- which are objects with an edge rather
than separators.

**Neutral, not cool** (settled 2026-07-30). An earlier pass gave these a
few points of blue, arguing that a mathematically neutral grey reads as
"default". Tanner rejected it, and on a screen that sits beside
Photoshop and VS Code all day he is right: the cast read as a tint
rather than as craft, and it fought every neutral surface next to it.

Neutral does not mean flat. The ladder does the work: four clearly
separated steps and seams darker than all of them. The harness asserts
the ladder ascends and that the cast stays within bounds -- a rule that
twice caught the cast drifting upward as the surfaces lightened, back
when there was one.

    --hairline-top   inset 0 1px 0 rgb(255 255 255 / 0.05)

One pixel of light along the top edge of raised surfaces: joinery.

Rules:
• Exactly one elevation step per context.
• Shadow belongs to things that genuinely float. Menus and dialogs
  float. Rows, panels and toolbars do not.
• No gradient on a surface. Removed 2026-07-30 at Tanner's request --
  a scoped decision for this app, not a claim that gradients are wrong.

======================================================================
2 — COLOUR: THE ACCENT MEANS LIVE
======================================================================

    --color-accent  #6aa8dc   live / selected / focused
    --color-signal  #c87a72   error, crash, destructive
    --color-warn    #d9a86c   in-between: stopping, restarting
    --color-good    #7fb08a   finished cleanly

The accent has exactly one meaning: **this is live right now.** Running
processes, the selected row, the focused control. It is never used to
draw attention to a feature, never used on a heading, never used because
a button looked plain.

That single restriction is what makes a running project findable in
peripheral vision. It only works if it is absolute — one decorative
accent anywhere in the window and the eye stops trusting the colour.

Contrast is deliberately *not* maximised. `#d6d6d6` ink on `#121212`
field measures **12.9:1** — comfortably past AAA, and materially calmer
than the ~17:1 of near-white on near-black. Past roughly 13:1 more
contrast buys nothing and costs eye strain over an hour.

The ink is neutral too (`#d6d6d6`, R=G=B). It briefly was not: while the
surfaces carried a cool cast the ink did as well, and darkening the
field then pushed the pair to 15.5:1 — past the comfort target this
paragraph argues for, by accident rather than by decision. Both were
corrected together.

Lower the contrast of the field, never the chroma of the accent. A
desaturated accent reads as washed-out, not calm.

Hierarchy is carried by weight and opacity, never by hue. Turn the
screen greyscale: every hierarchy must survive. Colour adds *state* on
top of a hierarchy that already reads.

**Where colour is allowed.**

Two passes were spent learning this. First every language got its own
muted hue on its icon tile -- read as "scattered", and correctly: twelve
hues down a list is confetti, and it put colour on the one surface that
should stay quiet. Then colour moved into the chrome as a blue-violet
gradient -- which made the accent a decoration you could not point at,
and gave the violet end no meaning of its own to carry.

What survives is the original rule, and it is enough:

    ONE accent, used FLAT, meaning exactly one thing: this is live.

Running processes, the primary action, the selected collection, the
focus ring. Never a heading, never a surface, never because a control
looked plain.

Interaction states are neutral steps of the ladder, not tinted washes.
The steps are far enough apart to carry hover and selection on their
own, and a tinted row competed with the accent edge that means running.

Icon tiles are one neutral treatment for every project.

======================================================================
2b — THE APP ICON
======================================================================

One flat WHITE mark on TRANSPARENCY. No background tile.

`src-tauri/icons/icon.svg` is the master, drawn on a 32-unit grid so
every straight edge lands on a whole unit. Rasters are generated from
that geometry, never drawn independently, so the vector and the shipped
`.ico` cannot drift.

    chevron  M16 4 L29 17 L24.5 21.5 L16 13 L7.5 21.5 L3 17 Z
    pad      x=9 y=24 w=14 h=4 rx=2

Why no tile: an earlier version put the mark on a filled blue rounded
square. On a dark taskbar beside a row of other coloured tiles that is
one more coloured square -- noise rather than identity.

Why a chevron and not a triangle over a bar: that silhouette is the
universal EJECT glyph and gets read as "eject" first. A rocket was drawn
and rejected; its fins merge into a blob by 16px.

CRISPNESS IS GEOMETRY, NOT RESOLUTION. The first attempt looked cheap
because it expressed geometry as fractions of the canvas -- at 24px a
"1.5% inset" is 0.36 of a pixel, so every straight edge smeared across
two and nothing in the icon had a crisp edge. Snap horizontal and
vertical edges to whole TARGET pixels before supersampling; let only the
diagonals antialias. Downsample with BOX, not LANCZOS, which rings
around high-contrast edges and haloes a white mark.

Ship `.ico` frames at 16/24/32/48/64/128/256, each drawn at its own
size. Windows downscaling one 256px frame to 16px is how icons turn to
mush.

VERIFY IT REACHED THE BINARY. `tauri-build` emits `rerun-if-changed` for
`tauri.conf.json` and `capabilities` only -- never the icon files -- so
replacing `icon.ico` does not invalidate the cached Windows resource.
Touch `tauri.conf.json`, then confirm by searching the exe for each ICO
frame's bytes. `ExtractAssociatedIcon` reads the shell icon cache and
will cheerfully show you the old icon.

The in-app mark in the toolbar uses the same path, with no plate behind
it, so the title bar and the taskbar agree on what this product's mark
is.

======================================================================
3 — TYPOGRAPHY: NUMBERS ARE INSTRUMENTS
======================================================================

One family, four sizes, three weights. More than that is decoration.

    13px   body, row content — the default
    12px   secondary values
    11px   metadata, column headers (uppercase, tracked)
    10px   badges only

Hierarchy order: **weight, then opacity, then size, then colour.**

Every changing number is tabular (`.tnum`). A CPU reading that reflows
as digits change width is unreadable at a glance, and this app's whole
job is glanceable numbers.

Number + unit is one indivisible token. `188 MB`, never `188` with `MB`
somewhere else, and never split across a line — enforced in CSS with
`whitespace-nowrap` on the cell rather than by hiding a U+00A0 inside
the string. An invisible character in a source file is something no
reviewer can see and any reformat can silently destroy.

Absent values render as an em dash, never as blank space or `0`. Blank
reads as a rendering failure; `0` is a claim the app cannot support.

Paths truncate from the *left* (`dir="rtl"`), because the informative
part of `D:\Workspace\Dev\Rust\Launch-Deck` is the tail.

======================================================================
4 — SPACING: 4px, NO EXCEPTIONS WITHOUT A REASON
======================================================================

    4  8  12  16  24  32  48  64

Row height is 48px: two lines of 13px text plus breathing room, and a
comfortable pointer target. It is fixed, so `content-visibility` can
reserve space correctly and so the eye can track across a row.

Alignment is not cosmetic — it is how a table is scanned:
• Labels and names: left.
• Changing values (CPU, RAM, uptime): **right**, so digits line up in a
  column and a spike is visible without reading.
• Actions: right edge, fixed width, always in the same place.

======================================================================
5 — RESPONSIVE: CONTAINER, NEVER VIEWPORT
======================================================================

The rule that took a real bug to learn, and the most important
paragraph in this document.

**Components respond to the width of their container, not the window.**

Those two numbers are different the moment a sibling panel opens. With
the log panel open on a 1240px window, the project list has ~470px. A
viewport breakpoint cheerfully reports "extra large" and renders all
seven columns into 470px — headers stack on top of each other and the
action buttons leave the visible area entirely.

Every breakpoint in the project list is therefore a container query
(`@container` + `@min-[Npx]:`), measured against the list itself. The
layout is then correct whether the window resized or a panel appeared —
two causes with one mechanism.

Responsive is a *priority order*, not shrinking. Declared explicitly:

    Never hidden    status dot, project name, primary action, overflow menu
    Sheds at 1000   Launched
    Sheds at 860    Uptime
    Sheds at 720    CPU / RAM
    Sheds at 560    Kind, and the Logs button
    Sheds at 480    the word "Status", and the Restart button

Two hard constraints on that table:

1. **Nothing becomes unreachable.** Every control that sheds has a home
   in the overflow menu. A button that is hidden at some window sizes
   and absent from the menu is a feature that silently does not exist.
2. **The name column never drops below 80px.** It is the only column
   that identifies the row; a table of unreadable names is not a
   degraded layout, it is a broken one.

Both are asserted by geometry in `layout-verify.mjs`, at four widths,
with the panel open and closed. Layout claims are cheap to make and
cheap to break, so they are tested rather than eyeballed.

======================================================================
6 — MOTION: EXPLAIN CHANGE, THEN STOP
======================================================================

    120ms   hover, press, colour
    160ms   panels, dialogs
    240ms   ceiling. Nothing in this app may exceed it.

Transitions name their properties — `transition-colors`, never
`transition-all`. `transition-all` animates properties nobody chose,
including layout ones, and turns a resize into a smear.

Only three things pulse, and only while genuinely transient:
`starting`, `stopping`, `restarting`. A pulse that outlives its
transition is an idle animation, and an idle animation in a window
that sits open all day is a distraction with no message.

`prefers-reduced-motion: reduce` removes all of it. Not shortened —
removed.

No bounce, no spring, no easing that overshoots. This is an instrument
panel, not a toy.

======================================================================
7 — STATE: EVERY COMPONENT OWES NINE ANSWERS
======================================================================

No component ships without: **default, hover, active, focus, disabled,
loading, empty, error, success.**

The ones actually skipped in practice, and what they must do here:

• **Loading** — a skeleton of the real shape, never a spinner in the
  middle of a panel. The layout must not jump when data lands.
• **Empty** — distinguish "nothing yet" from "nothing matched". A fresh
  install and a failed search are different situations and get different
  words plus the relevant next action.
• **Error** — say what failed and what to do. Show the real message from
  the backend; never replace it with a generic line. Every error carries
  a code and a retryable flag, and the UI only retries what is actually
  retryable.

**Focus is not optional and is never merely removed.** One treatment
across the app: a 2px accent outline. Where `outline` cannot work — a
table row, whose outline is clipped by the table — the replacement is an
*inset ring*, not a background change. A background change cannot
indicate focus on a row that is already selected, because selection uses
that same background. Focus and selection must be independently
readable, since keyboard users routinely have both at once on different
rows.

======================================================================
8 — PLATFORM: THIS IS A WINDOWS APPLICATION
======================================================================

It is built with web technology. It must not behave like a web page.

• `color-scheme` follows the app's own theme, so native scrollbars,
  carets and context menus are dark when the app is. A pale scrollbar
  welded to a charcoal panel is the most obvious tell there is — and it
  must track `data-theme`, not the OS, because the user can override.
• No text selection outside content meant to be copied. No rubber-band
  overscroll. Dialogs contain their own scrolling
  (`overscroll-behavior: contain`) so a trackpad flick cannot scroll the
  page behind them.
• Keyboard first: `Ctrl+K` to search, arrows to move, `Enter` to open.
• Destructive actions confirm. Removal names what is being removed.
• Nothing is hover-only. Hover may *emphasise* an action; it may never
  be the only way to discover one. A hover-gated control does not exist
  for a keyboard user and is invisible in a screenshot.

======================================================================
9 — WHAT IS ACTUALLY FORBIDDEN HERE
======================================================================

This section used to be a list of banned styles — gradients, glass, glow,
heavy shadow. That was wrong, and it was corrected on 2026-07-30: Tanner
never asked for a style ban, and a document that pretends he did narrows
every future decision for no reason.

**No style is forbidden.** What follows are defects — things that are
wrong regardless of which style the project wears.

    Meaningless icon           decoration wearing an icon's clothes
    Colour chosen at random    not from the token system
    Hierarchy only in colour   dies in greyscale, dies for ~8% of men
    Accent on something dead   spends the one colour that means live
    Motion outliving its cause an idle loop in a window open all day
    transition-all             animates properties nobody chose
    Hover-only control         invisible to keyboard, absent in stills
    Literal "..."              use an ellipsis character
    Spinner where a skeleton   hides the layout, then makes it jump
      belongs
    Emoji as an icon           renders differently per OS; not typography

The last one is the single standing *style* rule, and it is his, stated
outright: real SVG icons, never emoji.

**Why this app looks the way it does**, then, is a choice rather than a
restriction. It is a dense instrument used in short bursts, so it favours
flat surfaces, a tight type scale and motion that stops. A different
product for a different purpose would justify a completely different
answer, and that would not be a violation of anything.

======================================================================
10 — SHIP GATE
======================================================================

Mechanical checks — a build does not ship if any fails:

    □ tsc --noEmit clean, eslint --max-warnings 0 clean
    □ cargo clippy at pedantic, zero warnings
    □ layout-verify: geometry sound at 1600/1240/1024/900,
      panel open and closed
    □ No raw colour, radius, or duration outside tokens.css
    □ Every icon-only control has an aria-label
    □ Greyscale test: full hierarchy still readable

Judgement checks — slower, and the ones that actually matter:

    □ Would Linear, Stripe, or Ableton ship this?
    □ Is every accent on screen genuinely live?
    □ Can the next action be identified without reading a label?
    □ Does it look deliberate when nothing is running?
    □ Is there anything on screen that, removed, changes nothing?

The last question is the whole document compressed. If removing an
element costs nothing, it was decoration, and decoration is the thing
this app does not have.

======================================================================
11 — WHEN THESE RULES CONFLICT
======================================================================

They will. The order is fixed, so the argument is short:

    1. Correctness — never show a wrong or stale value
    2. Legibility  — never make the right value hard to read
    3. Reachability — never make a control unreachable
    4. Density     — then fit as much as possible
    5. Elegance    — then make it beautiful

Density loses to legibility: that is why the name column has a floor
and why columns shed instead of compressing. Elegance loses to
reachability: that is why a shed button gains a menu entry even though
the duplication is slightly inelegant.

Elegance is last, and it is still not optional. It is what "5" means,
not what "cut it" means.
