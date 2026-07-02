// Standalone assertions for src/lib/diff.ts — no test runner, no new deps.
//
// diff.ts is plain-JS-compatible TypeScript (types only), so we import it
// directly and let Node strip the types. Node >= 22.6 supports this via
// `--experimental-strip-types`; on 22.18+ / 23+ it is on by default. Run:
//
//   node --experimental-strip-types scripts/diff_test.mjs
//
// (the flag is a harmless no-op on versions where stripping is already on).
import { wordDiff, changeCount } from "../src/lib/diff.ts";

let failures = 0;
function check(label, cond) {
  if (cond) {
    console.log(`  ok  ${label}`);
  } else {
    console.error(`FAIL  ${label}`);
    failures++;
  }
}

// Whitespace-insensitive normalization for reconstruction invariants.
const norm = (s) => s.trim().replace(/\s+/g, " ");
const join = (segs, types) =>
  segs
    .filter((s) => types.includes(s.type))
    .map((s) => s.text)
    .join("");
// The kept + removed text must rebuild the raw; kept + added must rebuild the
// formatted. These hold for every case regardless of exact segmentation.
function invariants(label, raw, formatted) {
  const segs = wordDiff(raw, formatted);
  check(
    `${label}: equal+remove reconstructs raw`,
    norm(join(segs, ["equal", "remove"])) === norm(raw),
  );
  check(
    `${label}: equal+add reconstructs formatted`,
    norm(join(segs, ["equal", "add"])) === norm(formatted),
  );
  return segs;
}

// 1. Identical inputs -> a single equal segment carrying the whole text.
{
  const segs = wordDiff("Revisa todos los cambios", "Revisa todos los cambios");
  check("identical: single segment", segs.length === 1);
  check("identical: type equal", segs[0]?.type === "equal");
  check(
    "identical: text preserved",
    segs[0]?.text === "Revisa todos los cambios",
  );
}

// 2. Pure addition (empty raw) -> everything is an add.
{
  const segs = wordDiff("", "hello world");
  check("pure add: single segment", segs.length === 1);
  check("pure add: type add", segs[0]?.type === "add");
  check("pure add: text", segs[0]?.text === "hello world");
}

// 3. Pure removal (empty formatted) -> everything is a remove.
{
  const segs = wordDiff("hello world", "");
  check("pure remove: single segment", segs.length === 1);
  check("pure remove: type remove", segs[0]?.type === "remove");
  check("pure remove: text", segs[0]?.text === "hello world");
}

// 4. Mixed edit: leading filler dropped, rest kept. Structural + invariants.
{
  const raw = "Así que revisa todos los cambios";
  const formatted = "Revisa todos los cambios";
  const segs = invariants("mixed", raw, formatted);
  check("mixed: has a remove segment", segs.some((s) => s.type === "remove"));
  check("mixed: has an equal segment", segs.some((s) => s.type === "equal"));
}

// 5. Empty inputs -> no segments.
{
  const segs = wordDiff("", "");
  check("both empty: no segments", segs.length === 0);
}

// 6. Punctuation stays attached to its word (no split at comma/period).
{
  const segs = wordDiff("hello, world.", "hello, world.");
  check("punctuation: single equal segment", segs.length === 1);
  check(
    "punctuation: kept together",
    segs[0]?.text === "hello, world.",
  );
}

// 7. Interior insertion keeps surrounding words equal, invariants hold.
{
  const segs = invariants(
    "insertion",
    "the meeting is at five",
    "the team meeting is at five pm",
  );
  check("insertion: has add", segs.some((s) => s.type === "add"));
  check("insertion: has equal", segs.some((s) => s.type === "equal"));
}

// 8. changeCount powers the overlay card's "{n} Changes" header and its
// skip-when-unchanged guard, so it must count exactly the non-equal segments.
{
  check("changeCount: identical is 0", changeCount("hello world", "hello world") === 0);
  check("changeCount: both empty is 0", changeCount("", "") === 0);
  check("changeCount: pure add is 1", changeCount("", "hello world") === 1);
  check("changeCount: pure remove is 1", changeCount("hello world", "") === 1);
  const raw = "Así que revisa todos los cambios";
  const formatted = "Revisa todos los cambios";
  const segChanges = wordDiff(raw, formatted).filter((s) => s.type !== "equal")
    .length;
  check("changeCount: mixed matches non-equal segments", changeCount(raw, formatted) === segChanges);
  check("changeCount: mixed is at least 1", changeCount(raw, formatted) >= 1);
}

if (failures > 0) {
  console.error(`\n${failures} assertion(s) failed`);
  process.exit(1);
}
console.log("\nall diff assertions passed");
