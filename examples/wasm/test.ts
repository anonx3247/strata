// Calls the Strata functions of stats.strata, built into stats.wasm with
// `strata build --target wasm32`, through the typed wrappers of stats.ts.
// Run with `node test.ts` (or bun, or `deno run --allow-read test.ts`).
import { clamp, summary, grade, StrataError } from "./stats.ts";

const check = (what: string, ok: boolean) => {
  console.log((ok ? "ok   " : "FAIL ") + what);
  if (!ok) process.exitCode = 1;
};

check("clamp(15, 0, 10) is 10", clamp(15, 0, 10) === 10);
check("clamp(-3, 0, 10) is 0", clamp(-3, 0, 10) === 0);

const s = summary([3, 9, 6]);
check("summary of three scores", s.count === 3 && s.mean === 6 && s.best === 9);

try {
  summary([]);
  check("summary of none is an error", false);
} catch (e) {
  check("summary of none is an error", e instanceof StrataError && e.kind === "err" && e.value === "no scores");
}

check("a pass", JSON.stringify(grade(70, 50)) === '{"type":"Pass","margin":20}');
check("a fail", JSON.stringify(grade(40, 50)) === '{"type":"Fail","short_by":10}');
check("absent", JSON.stringify(grade(null, 50)) === '{"type":"Absent"}');

// a refinement on a parameter: an answer, not a fault
try {
  grade(-1, 50);
  check("a negative score is refused", false);
} catch (e) {
  check("a negative score is refused", e instanceof StrataError && e.kind === "arguments");
}

// a broken `requires`: a fault, thrown; the module starts again for the next call
try {
  clamp(1, 10, 0);
  check("clamp with lo > hi is a fault", false);
} catch (e) {
  check("clamp with lo > hi is a fault", e instanceof StrataError && e.kind === "fault" && /requires/.test(e.message));
}
check("calls work after a fault", clamp(5, 0, 10) === 5);

// the cost of a call: JSON both ways
const n = 20000, t0 = performance.now();
for (let i = 0; i < n; i++) clamp(i, 0, 100);
const us = ((performance.now() - t0) * 1000) / n;
console.error(`clamp: ${us.toFixed(2)} µs a call`);
