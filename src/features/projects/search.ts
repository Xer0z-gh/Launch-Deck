/**
 * Search scoring for the project list.
 *
 * The core loop is "find one project and press Run", so search is ranked,
 * not just filtered: the best match surfaces first and Enter takes it. Three
 * behaviors substring matching alone could not give:
 *
 * - **Field weighting** — a hit on the NAME outranks a hit in a path, so
 *   typing "fleet" puts Fleet above every project that merely lives under a
 *   folder mentioning it.
 * - **Position weighting** — a name PREFIX outranks a word start, which
 *   outranks a mid-word substring. "la" ranks Lathe/Launch-Deck above Vlad.
 * - **Subsequence fallback** — "lchd" still finds Launch-Deck. Typing the
 *   consonants of a name is faster than typing the name, and a daily tool
 *   should reward that. Subsequence hits rank below any substring hit and
 *   never apply to paths (a 4-letter query is a subsequence of half of every
 *   long path on disk — pure noise).
 *
 * Multi-word queries AND together: every term must match somewhere.
 */

import type { ProjectDto } from "@/lib/ipc";

/** Score tiers, spaced so a better KIND of match always beats a worse one. */
const NAME_PREFIX = 1000;
const NAME_WORD_START = 800;
const NAME_SUBSTRING = 600;
const FIELD_SUBSTRING = 400;
const PATH_SUBSTRING = 200;
const NAME_SUBSEQUENCE = 100;

/** Word starts: beginning, or after any of the separators names here use. */
function isWordStart(haystack: string, index: number): boolean {
  if (index === 0) return true;
  return /[\s\-_./\\]/.test(haystack[index - 1] ?? "");
}

/** `needle` appears in `haystack` in order, not necessarily adjacent. */
function isSubsequence(needle: string, haystack: string): boolean {
  let i = 0;
  for (const ch of haystack) {
    if (ch === needle[i]) i += 1;
    if (i === needle.length) return true;
  }
  return false;
}

/** One term's score against one project, or null when it does not match. */
function termScore(project: ProjectDto, term: string): number | null {
  const name = project.name.toLowerCase();

  const nameIdx = name.indexOf(term);
  if (nameIdx === 0) return NAME_PREFIX;
  if (nameIdx > 0) {
    return isWordStart(name, nameIdx) ? NAME_WORD_START : NAME_SUBSTRING;
  }

  const fields = [
    project.description ?? "",
    project.language,
    project.framework ?? "",
    project.category ?? "",
    ...project.tags,
  ];
  if (fields.some((f) => f.toLowerCase().includes(term))) return FIELD_SUBSTRING;
  if (project.root.toLowerCase().includes(term)) return PATH_SUBSTRING;

  // Subsequence only against the name, and only for 2+ characters — a single
  // letter is a subsequence of almost everything.
  if (term.length >= 2 && isSubsequence(term, name)) return NAME_SUBSEQUENCE;

  return null;
}

/**
 * Total score for a query, or null when any term fails to match.
 *
 * Ties inside a tier break on name length (shorter name = tighter match),
 * applied here as a small bonus so the caller can sort on the score alone.
 */
export function searchScore(project: ProjectDto, query: string): number | null {
  const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return 0;

  let total = 0;
  for (const term of terms) {
    const s = termScore(project, term);
    if (s === null) return null;
    total += s;
  }
  return total + Math.max(0, 50 - project.name.length);
}
