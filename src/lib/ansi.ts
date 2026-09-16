/**
 * Minimal, total ANSI SGR parser for the log viewer.
 *
 * Handles the sequences build tools actually emit -- colours (16, 256 and
 * truecolour), bold/dim/italic/underline, and reset -- and silently discards
 * every other escape (cursor movement, erase, titles). Total: no input can
 * throw, and unknown codes degrade to plain text rather than leaking escapes
 * into the DOM.
 */

export interface AnsiSpan {
  text: string;
  /** CSS colour, when a colour is active. */
  color?: string;
  background?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
}

/**
 * The standard 16-colour palette, tuned for a dark field. Indexes 0-7 normal,
 * 8-15 bright.
 */
const PALETTE16: readonly string[] = [
  "#3f4650", // black -- lifted so it stays visible on the dark field
  "#d97b72", // red
  "#8fbf8f", // green
  "#d9b36c", // yellow
  "#7aa8d9", // blue
  "#c193d1", // magenta
  "#72bfbf", // cyan
  "#c8cdd4", // white
  "#5c6570",
  "#e89a92",
  "#a8d9a8",
  "#e8cc8f",
  "#9cc0e8",
  "#d9b3e5",
  "#93d4d4",
  "#e8ecf0",
];

const ESC = "\u001b";
const BEL = "\u0007";

/** xterm 256-colour index to CSS colour. */
function color256(index: number): string {
  if (index < 16) return PALETTE16[index] ?? "#c8cdd4";
  if (index < 232) {
    const n = index - 16;
    const steps = [0, 95, 135, 175, 215, 255];
    const r = steps[Math.floor(n / 36) % 6] ?? 0;
    const g = steps[Math.floor(n / 6) % 6] ?? 0;
    const b = steps[n % 6] ?? 0;
    return `rgb(${r},${g},${b})`;
  }
  const grey = 8 + (index - 232) * 10;
  return `rgb(${grey},${grey},${grey})`;
}

interface Style {
  color?: string;
  background?: string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
}

function applySgr(style: Style, params: number[]): Style {
  const next = { ...style };
  let i = 0;
  while (i < params.length) {
    const p = params[i] ?? 0;
    switch (true) {
      case p === 0:
        return {};
      case p === 1:
        next.bold = true;
        break;
      case p === 2:
        next.dim = true;
        break;
      case p === 3:
        next.italic = true;
        break;
      case p === 4:
        next.underline = true;
        break;
      case p === 22:
        delete next.bold;
        delete next.dim;
        break;
      case p === 23:
        delete next.italic;
        break;
      case p === 24:
        delete next.underline;
        break;
      case p === 39:
        delete next.color;
        break;
      case p === 49:
        delete next.background;
        break;
      case p >= 30 && p <= 37:
        next.color = PALETTE16[p - 30] ?? "#c8cdd4";
        break;
      case p >= 90 && p <= 97:
        next.color = PALETTE16[p - 90 + 8] ?? "#c8cdd4";
        break;
      case p >= 40 && p <= 47:
        next.background = PALETTE16[p - 40] ?? "#c8cdd4";
        break;
      case p >= 100 && p <= 107:
        next.background = PALETTE16[p - 100 + 8] ?? "#c8cdd4";
        break;
      case p === 38 || p === 48: {
        // Extended colour: 38;5;n or 38;2;r;g;b
        const target = p === 38 ? "color" : "background";
        const mode = params[i + 1];
        if (mode === 5) {
          const idx = params[i + 2];
          if (idx !== undefined) next[target] = color256(idx);
          i += 2;
        } else if (mode === 2) {
          const [r, g, b] = [params[i + 2], params[i + 3], params[i + 4]];
          if (r !== undefined && g !== undefined && b !== undefined) {
            next[target] = `rgb(${r},${g},${b})`;
          }
          i += 4;
        }
        break;
      }
      default:
        break; // unknown code: ignore
    }
    i += 1;
  }
  return next;
}

/** Parses one log line into styled spans. */
export function parseAnsi(input: string): AnsiSpan[] {
  const spans: AnsiSpan[] = [];
  let style: Style = {};
  let text = "";

  const flush = () => {
    if (text.length > 0) {
      spans.push({ text, ...style });
      text = "";
    }
  };

  let i = 0;
  while (i < input.length) {
    const ch = input[i];
    if (ch !== ESC) {
      text += ch ?? "";
      i += 1;
      continue;
    }
    const next = input[i + 1];
    if (next === "[") {
      // CSI: collect until the final byte (@ through ~).
      let j = i + 2;
      let body = "";
      while (j < input.length) {
        const c = input[j];
        if (c !== undefined && c >= "@" && c <= "~") break;
        body += c ?? "";
        j += 1;
      }
      const final = input[j];
      if (final === "m") {
        flush();
        const params = body.length
          ? body.split(";").map((s) => Number.parseInt(s, 10) || 0)
          : [0];
        style = applySgr(style, params);
      }
      // Every other CSI (cursor, erase) is dropped.
      i = j + 1;
    } else if (next === "]") {
      // OSC: skip to BEL or ST (ESC backslash).
      let j = i + 2;
      while (j < input.length) {
        if (input[j] === BEL) {
          j += 1;
          break;
        }
        if (input[j] === ESC && input[j + 1] === "\\") {
          j += 2;
          break;
        }
        j += 1;
      }
      i = j;
    } else {
      // Lone ESC or two-char sequence: drop the ESC (and known followers).
      i += next !== undefined && "c78=>".includes(next) ? 2 : 1;
    }
  }
  flush();

  return spans.length > 0 ? spans : [{ text: "" }];
}
