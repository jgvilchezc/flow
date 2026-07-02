/**
 * Word-level diff for the history viewer. Compares a raw transcript against its
 * formatted version and reports which words were kept, added, or removed so the
 * UI can strike removals and highlight additions.
 *
 * The algorithm is a hand-rolled longest-common-subsequence over word tokens
 * (O(n·m), which is fine for dictation lengths). Punctuation stays attached to
 * its word — we tokenize only on whitespace — and consecutive tokens of the
 * same kind are merged into a single segment that preserves the original
 * spacing so the rendered text reads naturally.
 *
 * This file is intentionally written as plain-JS-compatible TypeScript (types
 * only, no TS-only runtime syntax) so `scripts/diff_test.mjs` can import it
 * directly under Node's type-stripping without a build step or extra deps.
 */

export type DiffSegment = {
  type: "equal" | "add" | "remove";
  text: string;
};

/** A word plus the whitespace that followed it in the source string. */
type Token = { word: string; sep: string };

/**
 * Splits a string into word tokens, keeping the trailing whitespace of each
 * word so segments can be reassembled with the original spacing. Leading
 * whitespace is dropped (dictation transcripts have none); punctuation is left
 * attached to the word it touches.
 */
function tokenize(str: string): Token[] {
  const tokens: Token[] = [];
  const re = /(\S+)(\s*)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(str)) !== null) {
    tokens.push({ word: m[1], sep: m[2] });
  }
  return tokens;
}

export function wordDiff(raw: string, formatted: string): DiffSegment[] {
  const a = tokenize(raw);
  const b = tokenize(formatted);
  const n = a.length;
  const m = b.length;

  // LCS length table: dp[i][j] is the LCS length of a[i..] and b[j..]. Filled
  // bottom-up so the forward walk below can pick the direction that preserves
  // the most shared words.
  const dp: number[][] = Array.from({ length: n + 1 }, () =>
    new Array<number>(m + 1).fill(0),
  );
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i][j] =
        a[i].word === b[j].word
          ? dp[i + 1][j + 1] + 1
          : Math.max(dp[i + 1][j], dp[i][j + 1]);
    }
  }

  type Op = { type: DiffSegment["type"]; token: Token };
  const ops: Op[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i].word === b[j].word) {
      ops.push({ type: "equal", token: b[j] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      ops.push({ type: "remove", token: a[i] });
      i++;
    } else {
      ops.push({ type: "add", token: b[j] });
      j++;
    }
  }
  while (i < n) {
    ops.push({ type: "remove", token: a[i] });
    i++;
  }
  while (j < m) {
    ops.push({ type: "add", token: b[j] });
    j++;
  }

  // Merge consecutive same-type tokens into one segment, keeping each token's
  // original trailing whitespace.
  const segments: DiffSegment[] = [];
  for (const op of ops) {
    const piece = op.token.word + op.token.sep;
    const last = segments[segments.length - 1];
    if (last && last.type === op.type) {
      last.text += piece;
    } else {
      segments.push({ type: op.type, text: piece });
    }
  }

  // Trim the trailing separator on the final segment so a whole-string /
  // identical reconstruction matches the input exactly.
  if (segments.length > 0) {
    const last = segments[segments.length - 1];
    last.text = last.text.replace(/\s+$/, "");
  }
  return segments;
}

/**
 * Number of non-equal segments between `raw` and `formatted` — the count the
 * overlay's post-dictation card shows as "{n} Changes". Zero means the
 * formatter left the text untouched, in which case the card is skipped.
 */
export function changeCount(raw: string, formatted: string): number {
  return wordDiff(raw, formatted).filter((s) => s.type !== "equal").length;
}
