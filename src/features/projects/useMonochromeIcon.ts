/**
 * Detects whether an icon is a white-on-transparency mark.
 *
 * # Why this exists
 *
 * Tanner's icon standard is "flat white mark on transparency, NO tile", which
 * assumes a dark ground. The moment the app grew a real iOS light theme --
 * where cards are pure white -- those marks became invisible: most rows in
 * the light theme had no icon at all, measured on screen.
 *
 * A blanket `invert()` in light mode is not the fix, because a genuinely
 * coloured icon (an app's own .ico, a screenshot-derived mark) would come out
 * as a photographic negative. So the question has to be answered per icon:
 * *is this mark monochrome and light?* Only those get inverted.
 *
 * The answer is computed from the decoded pixels in a canvas, once per icon
 * URL, and cached for the session -- an icon does not change while the app
 * runs. Anything unreadable (a tainted canvas, a decode failure) resolves to
 * `false`, i.e. leave the icon alone: the failure mode of a wrong `false` is
 * an icon that looks exactly as it does today.
 */
import { useEffect, useState } from "react";

/** Alpha below this is treated as transparent and ignored. */
const ALPHA_FLOOR = 24;
/** Channel spread above this means the pixel carries real colour. */
const MAX_CHROMA = 28;
/** Mean luminance above this is "light" -- the marks that vanish on white. */
const LIGHT_FLOOR = 170;
/** Share of opaque pixels that must be light and neutral to call it a mark. */
const AGREEMENT = 0.92;

const cache = new Map<string, boolean>();
const inFlight = new Map<string, Promise<boolean>>();

function analyse(url: string): Promise<boolean> {
  const running = inFlight.get(url);
  if (running) return running;

  const job = new Promise<boolean>((resolve) => {
    const img = new Image();
    img.onload = () => {
      try {
        // 32px is plenty to characterise a mark and costs nothing.
        const side = 32;
        const canvas = document.createElement("canvas");
        canvas.width = side;
        canvas.height = side;
        const ctx = canvas.getContext("2d", { willReadFrequently: true });
        if (!ctx) return resolve(false);
        ctx.drawImage(img, 0, 0, side, side);
        const { data } = ctx.getImageData(0, 0, side, side);

        let opaque = 0;
        let lightNeutral = 0;
        for (let i = 0; i < data.length; i += 4) {
          const a = data[i + 3] ?? 0;
          if (a < ALPHA_FLOOR) continue;
          opaque += 1;
          const r = data[i] ?? 0;
          const g = data[i + 1] ?? 0;
          const b = data[i + 2] ?? 0;
          const chroma = Math.max(r, g, b) - Math.min(r, g, b);
          const luma = 0.299 * r + 0.587 * g + 0.114 * b;
          if (chroma <= MAX_CHROMA && luma >= LIGHT_FLOOR) lightNeutral += 1;
        }
        // An icon that is entirely transparent tells us nothing; leave it.
        resolve(opaque > 0 && lightNeutral / opaque >= AGREEMENT);
      } catch {
        resolve(false);
      }
    };
    img.onerror = () => resolve(false);
    img.src = url;
  }).then((result) => {
    cache.set(url, result);
    inFlight.delete(url);
    return result;
  });

  inFlight.set(url, job);
  return job;
}

/**
 * `true` when `url` is a light monochrome mark that needs inverting on a
 * light surface. Returns `false` until the answer is known, so an icon never
 * flashes inverted and then corrects itself.
 */
export function useMonochromeIcon(url: string | null): boolean {
  const [mono, setMono] = useState(() => (url ? (cache.get(url) ?? false) : false));

  useEffect(() => {
    if (!url) {
      setMono(false);
      return;
    }
    const known = cache.get(url);
    if (known !== undefined) {
      setMono(known);
      return;
    }
    let alive = true;
    void analyse(url).then((result) => {
      if (alive) setMono(result);
    });
    return () => {
      alive = false;
    };
  }, [url]);

  return mono;
}
