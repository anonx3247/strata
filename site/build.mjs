// Builds the data behind the presentation (site/tour.html, and the landing page site/index.html) by running the
// real `strata` CLI: every command output and every snapshot of a codebase
// shown on the page (its definitions by `strata header`, verdicts by `verify
// --json`, holes by `holes --json`, changes by `diff --json`) is recorded
// here, nothing is written by hand.
//
//   tools/bootstrap.sh && node site/build.mjs   (or STRATA=PATH node site/build.mjs)
//
// Writes site/data/*.js (one `STRATA[...] = {...}` per scene). The docs are a
// site of their own, built by docs-site/.

import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, realpathSync, writeFileSync, readFileSync, readdirSync, existsSync, mkdirSync, rmSync, cpSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const SITE = dirname(fileURLToPath(import.meta.url));
const ROOT = dirname(SITE);
const STRATA = process.env.STRATA || join(ROOT, 'target/strata');
const EX = join(ROOT, 'examples');
const WORK = realpathSync(mkdtempSync(join(tmpdir(), 'strata-site-')));

/** Run strata in `cwd`, return its output as a terminal shows it (stdout and stderr, interleaved). */
function run(cwd, args, input) {
  const r = spawnSync('sh', ['-c', '"$0" "$@" 2>&1', STRATA, ...args], { cwd, input, encoding: 'utf8', env: { ...process.env, NO_COLOR: '1' } });
  // (what `verify` found, kept for the codebase's snapshots, as a viewer would show it)
  if (args[0] === 'verify' && !args.includes('--json')) keepVerdicts(cwd, args.slice(1));
  // (unused-definition warnings: true of a slide's small program, and noise on the slide)
  return { out: r.stdout.replace(/^warning: .* is never (used|read|built)\b.*\n/gm, ''), code: r.status };
}
/** Only what strata writes on standard output, as JSON. */
function json(cwd, args) {
  const r = spawnSync(STRATA, args, { cwd, encoding: 'utf8', maxBuffer: 1 << 28, env: { ...process.env, NO_COLOR: '1' } });
  try { return JSON.parse(r.stdout); } catch { throw new Error(`strata ${args.join(' ')} in ${cwd}: no JSON:\n${r.stdout}${r.stderr}`); }
}
function git(cwd, ...args) { return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }); }
// (`(cached)`: what an earlier build verified, which a first run on the reader's machine wouldn't say)
const clean = s => s.replaceAll(WORK, '~').replaceAll(' (cached)', '').replace(/\s+$/, '');

/** A package's module files (`main.strata`, `shop/orders.strata`), and the module each holds. */
function modules(dir, sub = '') {
  const out = [];
  for (const e of readdirSync(join(dir, sub), { withFileTypes: true })) {
    if (e.name.startsWith('.')) continue;
    const rel = sub ? `${sub}/${e.name}` : e.name;
    if (e.isDirectory()) out.push(...modules(dir, rel));
    else if (e.name.endsWith('.strata')) out.push({ file: rel, module: rel.replace(/\.strata$/, '').replaceAll('/', '.') });
  }
  return out.sort((a, b) => a.file.localeCompare(b.file));
}

/** Top-level definitions of printed source, in order, each with its doc comments. */
function chunks(src) {
  const cs = [];
  for (const l of src.trimEnd().split('\n')) {
    if (!cs.length || (l && !/^[\s}]/.test(l) && cs.at(-1).at(-1) === '' && !cs.at(-1).every(x => x === '' || x.startsWith('//')))) cs.push([]);
    cs.at(-1).push(l);
  }
  return cs.map(c => c.join('\n').trim());
}

/** Each definition of the package as `strata header` lists it: kind, name, hashes, what it uses, its text (no docs). */
function definitions(dir) {
  const out = [];
  for (const { file, module } of modules(dir)) {
    const q = n => module === 'main' ? n : `${module}.${n}`;
    const texts = chunks(run(dir, ['header', file, '--text']).out);
    const lines = run(dir, ['header', file]).out.split('\n').slice(1);
    const ds = [];
    for (const l of lines) {
      let m;
      if ((m = /^(\S+)\s+(.+)$/.exec(l))) ds.push({ kind: m[1], name: q(m[2].trim()), deps: [] });
      else if ((m = /^\s+#(\w+)\s+interface #(\w+)/.exec(l))) Object.assign(ds.at(-1), { hash: m[1], interface_hash: m[2] });
      else if ((m = /^\s+uses (.+)$/.exec(l))) ds.at(-1).deps = m[1].split(', ').map(q);
    }
    if (ds.length !== texts.length) throw new Error(`${file}: ${ds.length} definitions, ${texts.length} texts`);
    ds.forEach((d, i) => { d.text = texts[i].split('\n').filter((l, k, all) => !(l.startsWith('///') && all.slice(0, k).every(x => x.startsWith('///')))).join('\n'); });
    out.push(...ds);
  }
  return out;
}

/** The verdicts `verify` gave in each package, by function, with the hash of the definition they are about. */
const VERDICTS = new Map();
/** The definition a verdict's function is part of: itself, or the `impl` of its type (`MemDb.load`). */
function ownerOf(ds, fn) {
  const own = ds.find(d => d.name === fn);
  if (own) return own;
  return ds.find(d => d.kind === 'impl' && fn.startsWith(d.name.replace(/^(\w+\.)?impl .* for /, '$1') + '.'));
}
function keepVerdicts(dir, sel) {
  if (!existsSync(join(dir, 'strata.toml'))) return;
  const ds = definitions(dir);
  const kept = VERDICTS.get(dir) || new Map();
  for (const { fn, verdict } of json(dir, ['verify', ...sel, '--json'])) {
    const d = ownerOf(ds, fn);
    if (d) kept.set(fn, { owner: d.name, hash: d.hash, verdict });
  }
  VERDICTS.set(dir, kept);
}
/** The worst of a definition's verdicts (an `impl`'s methods'), still about what it is now. */
const WORST = ['counterexample', 'incomplete', 'skipped', 'fuzzed', 'assumed', 'proven'];
function verdictOf(dir, d) {
  const vs = [...(VERDICTS.get(dir)?.values() || [])].filter(v => v.owner === d.name && v.hash === d.hash).map(v => v.verdict);
  vs.sort((a, b) => WORST.indexOf(a.status) - WORST.indexOf(b.status));
  return vs[0] ? { status: vs[0].status, args: vs[0].args, message: vs[0].message } : null;
}

/** A package as a viewer showed it, made of what strata's commands print: its definitions, and its Changes view. */
function codebase(dir, label = '~/' + dir.split('/').pop()) {
  return {
    /** The definitions, what each uses, its holes, verdict and text. */
    state() {
      const holes = json(dir, ['holes', '--json']);
      const decls = definitions(dir).map(d => ({
        ...d,
        label: d.kind === 'test' ? d.name.replace(/^(\w+\.)?test "(.*)"$/, '$2') : d.name,
        holes: holes.filter(h => h.decl === d.name),
        verdict: verdictOf(dir, d),
        text: { 2: d.text },
      }));
      return { label, decls };
    },
    /** The state, and the Changes view: all steps (`strata diff --json`), and each step. */
    snap() {
      const state = this.state();
      const all = json(dir, ['diff', '--json']);
      const changes = { all };
      // (`--step K` counts as `strata diff` shows the steps: by patch, from 1)
      for (const s of all.steps || []) changes[s.id] = json(dir, ['diff', '--step', String(s.id + 1), '--json']);
      return { state, changes };
    },
    stop() {},
  };
}

function pkg(name, files) {
  const dir = join(WORK, name);
  mkdirSync(dir, { recursive: true });
  git(dir, 'init', '-q', '-b', 'main');
  git(dir, 'config', 'user.name', 'agent');
  git(dir, 'config', 'user.email', 'agent@example.com');
  run(dir, ['init']);
  for (const f of files) run(dir, ['add', f]);
  return dir;
}

const OUT = join(SITE, 'data');
rmSync(OUT, { recursive: true, force: true });
mkdirSync(OUT, { recursive: true });
function emit(name, value) {
  writeFileSync(join(OUT, `${name}.js`), `// generated by site/build.mjs: do not edit\nSTRATA[${JSON.stringify(name)}] = ${JSON.stringify(value)};\n`);
  console.log(`  data/${name}.js  ${(JSON.stringify(value).length / 1024).toFixed(0)} KB`);
}

/** The codebase as the page draws it: each definition, what it uses, its status, its source. */
function graph(state) {
  const names = new Set(state.decls.map(d => d.name));
  return state.decls.map(d => ({
    name: d.name, label: d.label, kind: d.kind, hash: d.hash, interface: d.interface_hash,
    deps: [...new Set(d.deps)].filter(n => names.has(n) && n !== d.name),
    holes: d.holes.length, verdict: d.verdict?.status ?? null,
    args: d.verdict?.args ?? null, message: d.verdict?.message ?? null,
    text: d.text?.['2'] ?? d.text?.['1'] ?? '',
  }));
}

// ───────────────────────── math: the verifier's tour ─────────────────────────
{
  const dir = pkg('math', [join(EX, 'math.strata')]);
  const verify = run(dir, ['verify']);
  const v = codebase(dir);
  emit('math', { verify: clean(verify.out), graph: graph((await v.snap()).state) });
  v.stop();
}

// ─────────────── orders: levels of detail, holes, a trace, the agent session ───────────────
/** What `git diff` would show for the module, were it a text file. */
function lineDiff(a, b) {
  if (a === b) return null;
  const fa = join(WORK, 'a.txt'), fb = join(WORK, 'b.txt');
  writeFileSync(fa, a); writeFileSync(fb, b);
  const r = spawnSync('diff', ['-u', '--label', 'a/main.strata', '--label', 'b/main.strata', fa, fb], { encoding: 'utf8' });
  return r.stdout.trimEnd();
}
const ST = (s) => s.replace(/^\n/, '');
/** `strata run total`'s argument, as code. */
const TRACE_ARG = '[Line { sku: "tea", qty: 2, unit_price: 450 }, Line { sku: "cup", qty: 1, unit_price: 1200 }]';
const session = [];
const texts = []; // [before, after] of each step, as text
{
  const dir = pkg('shop', [join(EX, 'orders.strata')]);
  const v = codebase(dir);
  const levels = {};
  for (const l of [0, 1, 2]) levels[l] = run(dir, ['view', 'slice:Order', '--level', String(l)]).out;
  const trace = run(dir, ['run', 'total', TRACE_ARG, '--trace']);
  const badInput = run(dir, ['run', 'total', '[]']);

  /** One step of the session: a command (optionally with what was piped into it), its real output, the viewer after it. */
  async function step(s) {
    const before = run(dir, ['view', 'main']).out;
    const r = s.args ? run(dir, s.args, s.input) : { out: '', code: 0 };
    for (const t of s.then || []) r.out += '\n$ strata ' + t.join(' ') + '\n' + run(dir, t).out;
    if (s.hidden) for (const h of s.hidden) run(dir, h.args, h.input);
    const snap = await v.snap();
    const after = run(dir, ['view', 'main']).out;
    texts.push([before, after]);
    const linediff = lineDiff(before, after);
    session.push({ linediff, ...s, args: undefined, hidden: undefined, then: undefined, cmd: s.cmd ?? `strata ${s.args.join(' ')}`, input: s.input && ST(s.input), out: clean(r.out), code: r.code, ...snap });
    console.log(`  step ${session.length}: ${s.title}${r.code ? ` (exit ${r.code})` : ''}`);
  }

  await step({ who: 'you', title: 'A codebase, checked and verified', sel: 'refund', cmd: 'strata verify', args: ['verify'],
    say: 'An order system: refinement types, a `Db` capability, contracts. Everything proven, except `refund`: it is a hole.' });
  await step({ who: 'agent', title: 'The agent asks what is left to do', sel: 'refund', args: ['holes'],
    say: 'A hole comes with what is needed to fill it: the type to produce, what is in scope, and the contract the code must meet.' });
  await step({ who: 'agent', title: 'It fills the hole', sel: 'refund', args: ['edit', '-m', 'refund policy: fee of 5.00'], cmd: 'strata edit -m "refund policy: fee of 5.00"',
    input: `
/// Refund what was paid, minus a 5.00 handling fee.
fn refund(o: Order) -> Cents {
  match o {
    Pending { lines, .. } => total(lines) - 500,
    Shipped { .. } => 0,
    Cancelled { .. } => 0,
  }
}

test "refund keeps the handling fee" {
  line := Line { sku: "tea", qty: 2, unit_price: 450 }
  assert refund(Pending { id: "ord_abc123", lines: [line] }) == 400
}
`, say: 'With a test, as a careful agent would. The edit is checked against everything it touches, then committed. Only the implementation changed, so nothing that uses `refund` needs another look.' });
  await step({ who: 'agent', title: 'The tests pass', sel: 'refund', args: ['test'],
    say: 'Every test passes. They check the orders someone thought of: a 9.00 order, refunded 4.00.' });
  await step({ who: 'agent', title: 'Verification finds the bug', sel: 'refund', args: ['verify', 'refund'],
    say: 'A refund is `Cents`, which is never below zero, for every order. The solver and the fuzzer look for one that breaks it: an order worth less than the fee would get a negative refund. The input that breaks it comes with the verdict.' });
  await step({ who: 'agent', title: 'It fixes it, and proves it', sel: 'refund', args: ['edit', '-m', 'refund: never below zero'], cmd: 'strata edit -m "refund: never below zero"',
    input: `
/// Refund what was paid, minus a 5.00 handling fee, never below zero.
fn refund(o: Order) -> Cents {
  match o {
    Pending { lines, .. } => max(total(lines) - 500, 0),
    Shipped { .. } => 0,
    Cancelled { .. } => 0,
  }
}
`, then: [['verify', 'refund']], say: 'Fixed, then `strata verify`: proven for every order.' });
  await step({ who: 'agent', title: 'A breaking change becomes holes', sel: 'refund', args: ['edit', '-m', 'coupons'], cmd: 'strata edit -m "coupons"',
    input: `
fn total(lines: NonEmpty[Line], coupon: Cents) -> Cents {
  max(sum(lines.map(line_total)) - coupon, 0)
}
`, say: '`total` now takes a coupon. Its two callers no longer check, so they become holes in a pending change, each with what it was and why it broke.' });
  await step({ who: 'agent', title: 'It fills them, and closes the change', sel: 'refund', cmd: 'strata edit', args: ['edit'],
    input: `
/// Refund what was paid, minus a 5.00 handling fee, never below zero.
fn refund(o: Order) -> Cents {
  match o {
    Pending { lines, .. } => max(total(lines, 0) - 500, 0),
    Shipped { .. } => 0,
    Cancelled { .. } => 0,
  }
}

test "totals" {
  lines := [
    Line { sku: "a", qty: 3, unit_price: 100 },
    Line { sku: "b", qty: 1, unit_price: 50 },
  ]
  assert total(lines, 0) == 350
  assert total(lines, 100) == 250
  assert discount(20000) == 18000
}
`, then: [['done']], hidden: [{ args: ['verify'] }], say: 'One logical change, recorded as one entry in the log and one git commit: three definitions.' });
  await step({ who: 'agent', title: 'A rename is one change', sel: 'Order', args: ['rename', 'Shipped', 'Sent'],
    say: 'Names are resolved in the tree, so renaming a variant updates every pattern, literal and test that uses it, and nothing else.' });
  await step({ who: 'agent', title: 'A broken edit is refused', sel: 'discount', args: ['edit'], cmd: 'strata edit',
    input: `
fn discount(amount: Cents) -> Cents {
  amount - "ten"
}
`, say: 'Nothing that doesn\'t check gets in. The codebase is exactly as it was.' });

  // Two agents, two branches: B rewrites line_total, main renames it.
  git(dir, 'checkout', '-q', '-b', 'agent-b');
  const b = run(dir, ['edit', '-m', 'b: tidy line_total'], 'fn line_total(l: Line) -> Cents\n  ensures it == l.qty * l.unit_price {\n  l.unit_price * l.qty\n}\n');
  git(dir, 'checkout', '-q', 'main');
  const a = run(dir, ['rename', 'line_total', 'line_cost']);
  git(dir, 'checkout', '-q', 'agent-b');
  await step({ who: 'agent B', title: 'Two agents, one merge', sel: 'line_cost', args: ['sync', 'main'],
    say: 'Agent A renamed `line_total` on main while agent B rewrote its body on a branch. Git merges definition by definition, and the rename follows into B\'s code.' });
  const merge = {
    a: { cmd: 'strata rename line_total line_cost', out: clean(a.out) },
    b: { cmd: 'strata edit -m "b: tidy line_total"', input: 'fn line_total(l: Line) -> Cents\n  ensures it == l.qty * l.unit_price {\n  l.unit_price * l.qty\n}', out: clean(b.out) },
    sync: session.at(-1).out,
    graph: git(dir, 'log', '--oneline', '--graph').trim(),
    result: run(dir, ['view', 'line_cost,total', '--level', '2']).out.trim(),
  };
  // Only the merge's snapshot, without its large Changes history, is needed beyond this.
  v.stop();

  const holes = session[1].out;
  const at = t => session.findIndex(x => x.title === t);
  const couponsDiff = lineDiff(texts[at('A breaking change becomes holes')][0], texts[at('It fills them, and closes the change')][1]);
  emit('orders', { traceCmd: `strata run total '${TRACE_ARG}' --trace`, couponsDiff, levels, trace: clean(trace.out), badInput: clean(badInput.out), holes, merge, source: levels[3] });
  // the page draws each step's codebase as a graph; it needs the Changes view only for the diffs it shows
  const DIFFS = ['It fills them, and closes the change', 'A rename is one change', 'It fixes it, and proves it'];
  emit('session', session.map(x => ({ ...x, state: undefined, changes: DIFFS.includes(x.title) ? x.changes : undefined, graph: graph(x.state) })));
}

// ─────────────── contracts: tighten one, then change a signature ───────────────
{
  const dir = pkg('contract', [join(EX, 'orders.strata')]);
  // the example's `?refund_policy` hole is another story: fill it, so the only holes are this one's
  run(dir, ['edit', '-m', 'refund: nothing yet'], 'fn refund(o: Order) -> Cents {\n  0\n}\n');
  const discount = (params, req) => `/// 10% off orders of 100.00 or more.
fn discount(${params}) -> Cents
${req ? '  requires amount >= 100\n' : ''}  ensures it <= amount {
  if amount >= 10000 {
    amount - amount / 10
  } else {
    amount
  }
}
`;
  const checkout = call => `/// What the customer pays: the total, with the discount applied.
fn checkout(lines: NonEmpty[Line]) -> Cents {
  t := total(lines)
  if t >= 100 {
    ${call}
  } else {
    t
  }
}
`;
  const steps = [];
  const texts = [];
  const cv = codebase(dir);
  /** One step: a command, what was piped into it, and the commands run right after it; the codebase after it. */
  async function step(s) {
    const before = run(dir, ['view', 'main']).out;
    const r = run(dir, s.args, s.input);
    let out = clean(r.out);
    for (const t of s.then || []) out += '\n\n$ strata ' + t.join(' ') + '\n' + clean(run(dir, t).out);
    const after = run(dir, ['view', 'main']).out;
    texts.push([before, after]);
    steps.push({ ...s, args: undefined, then: undefined, cmd: s.cmd ?? `strata ${s.args.join(' ')}`, input: s.input?.trim(), out, linediff: lineDiff(before, after), graph: graph(await cv.state()) });
    console.log(`  contract ${steps.length}: ${s.title}`);
  }
  await step({ title: 'Today, checkout is proven', g: { checkout: 'pass' }, args: ['edit', '-m', 'checkout'], cmd: 'strata edit -m "checkout"', then: [['verify', 'checkout']],
    input: '/// What the customer pays: the total, with the discount applied.\nfn checkout(lines: NonEmpty[Line]) -> Cents {\n  discount(total(lines))\n}\n',
    say: '`checkout` applies `discount` to the total. Verified: proven.' });
  await step({ title: 'Tighten discount’s contract', g: { discount: 'changed', checkout: 'recheck', totals: 'recheck', lit: true }, args: ['edit', '-m', 'discount only from 1.00'], cmd: 'strata edit -m "discount only from 1.00"',
    input: discount('amount: Cents', true),
    say: 'One new line: `requires amount >= 100`. The interface changed, so the edit names what depends on it and must be re-checked.' });
  await step({ title: 'Verification finds the caller that breaks', g: { discount: 'changed', checkout: 'fail', totals: 'pass', bad: true }, args: ['verify', 'checkout'],
    say: '`checkout` is checked against the new contract, and the verdict comes with an input that breaks it: a basket under 1.00.' });
  await step({ title: 'Fix the caller', g: { discount: 'changed', checkout: 'pass', totals: 'pass' }, args: ['edit', '-m', 'checkout: no discount under 1.00'], cmd: 'strata edit -m "checkout: no discount under 1.00"', then: [['verify', 'checkout']],
    input: checkout('discount(t)'), say: '`checkout` now applies the discount only from 1.00. Proven again.' });
  await step({ title: 'Add a parameter: the callers become holes', g: { discount: 'changed', checkout: 'hole', totals: 'hole', lit: true }, args: ['edit', '-m', 'discount: members'], cmd: 'strata edit -m "discount: members"',
    input: discount('amount: Cents, member: Bool', true),
    say: '`discount` takes a `member` flag. Its two calls no longer check: rather than refusing the edit, Strata turns them into holes in a pending change.' });
  await step({ title: 'See where the holes are', g: { discount: 'changed', checkout: 'hole', totals: 'hole' }, args: ['holes'],
    say: 'Each hole says what it was, why it broke, and what is in scope to fill it, for you or an agent.' });
  await step({ title: 'Fill them, and close the change', g: { discount: 'changed', checkout: 'recheck', totals: 'recheck' }, args: ['edit'], cmd: 'strata edit', then: [['done']],
    input: checkout('discount(t, false)') + `
test "totals" {
  lines := [
    Line { sku: "a", qty: 3, unit_price: 100 },
    Line { sku: "b", qty: 1, unit_price: 50 },
  ]
  assert total(lines) == 350
  assert discount(20000, false) == 18000
}
`, say: '`strata done` records it all as one change, one commit: the new parameter, and the two calls that follow it.' });
  await step({ title: 'Verify everything', g: { discount: 'pass', checkout: 'pass', totals: 'pass' }, args: ['verify'],
    say: 'Every contract holds again, for every input.' });
  const at = t => steps.findIndex(x => x.title === t);
  const paramDiff = lineDiff(texts[at('Add a parameter: the callers become holes')][0], texts[at('Fill them, and close the change')][1]);
  emit('contract', { steps, paramDiff, changes: (await cv.snap()).changes });
  cv.stop();
}

// ─────────────── a rename in a larger codebase: what a line diff makes of it ───────────────
{
  const dir = join(WORK, 'tasks');
  cpSync(join(EX, 'tasks'), dir, { recursive: true });
  git(dir, 'init', '-q', '-b', 'main');
  git(dir, 'config', 'user.name', 'agent');
  git(dir, 'config', 'user.email', 'agent@example.com');
  git(dir, 'add', '-A'); git(dir, 'commit', '-qm', 'tasks');
  const mods = ['model', 'memory', 'print'];
  const text = () => mods.map(m => run(dir, ['view', m]).out).join('\n');
  const before = text();
  const r = run(dir, ['rename', 'Task', 'Job']);
  if (r.code) throw new Error('rename Task Job: ' + r.out);
  const diff = lineDiff(before, text()).replace('a/main.strata', 'a/tasks').replace('b/main.strata', 'b/tasks');
  const v = codebase(dir);
  const snap = await v.snap();
  v.stop();
  const step = snap.changes.all.steps.at(-1).id;
  emit('rename', { cmd: 'strata rename Task Job', out: clean(r.out), updated: +(/updated (\d+) definitions/.exec(r.out)?.[1] ?? 0), modules: mods.length, diff, changes: { all: snap.changes.all, [step]: snap.changes[step] }, step });
}

// ─────────────── what the language does now: small programs, stored, run and verified ───────────────
/** Top-level definitions of rendered source by name (with their doc comments), in source order. */
function defs(src, names) {
  const chunks = [];
  for (const l of src.trimEnd().split('\n')) {
    if (!chunks.length || (l && !/^[\s}]/.test(l) && chunks.at(-1).at(-1) === '' && !chunks.at(-1).every(x => x === '' || x.startsWith('//')))) chunks.push([]);
    chunks.at(-1).push(l);
  }
  const name = c => (c.find(l => !l.startsWith('//') && l) || '').replace(/^(pub |extern c |capability |mock |distinct )*/, '');
  return chunks.filter(c => names.some(n => name(c).startsWith(n))).map(c => c.join('\n').trim()).join('\n\n');
}
/** `strata verify`'s lines about the package's own definitions (std's derived helpers left out). */
const own = o => {
  const out = []; let skip = false;
  for (const l of o.out.split('\n')) {
    if (/^\S/.test(l)) skip = /^[✓✗~≈○-] std\./.test(l);
    if (!skip) out.push(l);
  }
  return { ...o, out: out.join('\n') };
};
{
  const F = {};
  const ex = name => { const dir = join(WORK, 'ex-' + name); cpSync(join(EX, name), dir, { recursive: true }); return dir; };
  const out = (dir, args, input) => { const r = run(dir, args, input); return { cmd: `strata ${args.join(' ')}`, out: clean(r.out), code: r.code }; };
  /** A small package: the program stored with `strata edit` (which checks it), then shown as stored. */
  const file = (name, text) => {
    const dir = pkg('f-' + name, []);
    const r = run(dir, ['edit', '-m', name], text);
    if (r.code) throw new Error(`${name} doesn't check:\n${r.out}`);
    return { dir, src: run(dir, ['view', 'main']).out.trimEnd(), at: (...args) => out(dir, args) };
  };
  const unfile = o => o;

  // capabilities: main names what it touches; the rest takes any implementation
  {
    const dir = ex('wc');
    const src = run(dir, ['view', 'main']).out;
    writeFileSync(join(dir, 'notes.txt'), 'one two\nthree\n');
    F.capabilities = {
      main: defs(src, ['fn main(']), wc: defs(src, ['fn wc(']), test: defs(src, ['test "counts each file']),
      test_out: out(dir, ['test']), run_out: out(dir, ['run', '--', '--only', 'words', 'notes.txt']), verify_out: own(out(dir, ['verify'])),
    };
  }
  // mocks: a fake declared as a type; what it leaves out is generated
  {
    const f = file('mocks', `from std.fs use Files, FsError

/// Save a note, then read it back.
fn round_trip(mut fs: Files, text: String) -> Result[String, FsError] {
  mut fs.write("note.txt", text)!
  fs.read("note.txt")
}

/// An in-memory disk, for tests.
mock MemDisk = { files: Dict[String, String] = Dict[String, String] {} }

impl Files for MemDisk {
  fn read(self, path: String) -> Result[String, FsError] {
    Ok(get(self.files, path) ?? "")
  }

  fn write(mut self, path: String, text: String) -> Result[(), FsError] {
    mut self.files.insert(path, text)
    Ok(())
  }
}

test "saving then loading" {
  mut fs := MemDisk {}
  assert round_trip(mut fs, "hi") == Ok("hi")
}
`);
    const main = 'fn main() {\n  mut fs := MemDisk {}\n  print(round_trip(mut fs, "hi"))\n}\n';
    F.mocks = { src: f.src, test_out: f.at('test'), verify_out: f.at('verify'), refused: { cmd: 'strata edit', input: main.trim(), ...out(f.dir, ['edit'], main), cmd: 'strata edit' } };
  }
  // derive: code written by code, at compile time
  {
    const dir = ex('hello');
    const src = run(dir, ['view', 'main']).out;
    const json = file('json', `from std.json use Json, parse, stringify

type Signup = {
  email: String where contains(it, "@"),
  age: Int where it >= 13,
  nickname: Option[String],
}

derive Json for Signup

/// Read a signup from JSON text: parsed, then checked against the type.
fn read(text: String) -> String {
  json := parse(text) except e {
    return f"not JSON: {e}"
  }
  match Signup.from_json(json) {
    Ok(s) => f"read: {s}",
    Err(e) => f"refused: {e}",
  }
}

/// Write a signup as JSON text.
fn write(s: Signup) -> String {
  match s.to_json() {
    Ok(json) => stringify(json),
    Err(e) => f"can't write: {e}",
  }
}

fn main() {
  text := write(Signup { email: "ada@example.com", age: 36, nickname: Some("ada") })
  print(f"written: {text}")
  print(read(text))
  print(read("{\\"email\\": \\"bob@example.com\\", \\"age\\": 9}"))
  print(read("{\\"email\\": \\"bob\\", \\"age\\": 40}"))
}
`);
    F.derive = {
      options: defs(src, ['type Options', 'derive CommandLine']), main: defs(src, ['fn main(']),
      help: out(dir, ['run', '--', '--help']), run_out: out(dir, ['run', '--', 'Ada', '--loud', '--times', '2']), bad: out(dir, ['run', '--', '--times', '11']),
      json: json.src.replace(/^use [^\n]*\n\n/, ''), json_run: unfile(json.at('run')),
    };
  }
  // comptime, and C called from Strata
  {
    const f = file('comptime', `fn fib(n: Natural) -> Natural {
  if n < 2 {
    n
  } else {
    fib(n - 1) + fib(n - 2)
  }
}

/// Worked out while compiling: the program only carries the number.
fn answer() -> Natural {
  comptime fib(30)
}

fn main() {
  print(answer())
}
`);
    const dir = ex('fuzzy');
    F.native = {
      comptime: f.src, comptime_run: unfile(f.at('run')),
      externs: run(dir, ['view', 'text']).out.trimEnd(),
      fuzzy_run: out(dir, ['run', '--', '--word', 'recieve']), fuzzy_verify: own(out(dir, ['verify'])),
    };
  }
  // std written in Strata: the regex parser
  {
    const f = file('regex', `from std.regex use parse, show

fn main() {
  match parse("(?i)ab|ac") {
    Ok(re) => print(show(re)),
    Err(e) => print(f"error: {e}"),
  }
}
`);
    F.std = { src: f.src, run_out: unfile(f.at('run')) };
  }
  for (const [k, v] of Object.entries(F)) console.log(`  feature ${k}`);
  emit('features', F);
}

// ─────────────── verification, on small programs ───────────────
{
  /** A small package from text: stored with `strata edit`, which checks it. */
  const prog = (name, text) => {
    const dir = pkg('v-' + name, []);
    const r = run(dir, ['edit', '-m', name], text);
    if (r.code) throw new Error(`${name} doesn't check:\n${r.out}`);
    return dir;
  };
  const out = (dir, args, input) => { const r = run(dir, args, input); return { cmd: `strata ${args.join(' ')}`, out: clean(r.out), code: r.code }; };
  const V = {};
  // what a contract says, and what the solver does with it
  {
    const dir = prog('contracts', `type Cents = Int
  where it >= 0

type Percent = Int
  where 0 <= it && it <= 100

/// A price after taking a percentage off.
fn discount(price: Cents, pct: Percent) -> Cents
  requires price >= 100
  ensures it <= price {
  price - price * pct / 100
}

/// What the customer pays.
fn checkout(price: Cents) -> Cents {
  if price >= 100 {
    discount(price, 10)
  } else {
    price
  }
}

/// Half price, on everything.
fn sale(price: Cents) -> Cents {
  discount(price, 50)
}
`);
    const src = run(dir, ['view', 'main']).out;
    V.contracts = { types: defs(src, ['type Cents', 'type Percent']), discount: defs(src, ['fn discount']), callers: defs(src, ['fn checkout', 'fn sale']), verify: out(dir, ['verify']) };
  }
  // tests that pass, and the detail they missed
  {
    const dir = prog('shares', `type Cents = Int
  where it >= 0

/// Split a bill between people.
fn shares(total: Cents, people: Positive) -> List[Cents] {
  [total / people for _ in 0..people]
}

test "splits a bill evenly" {
  assert shares(900, 3) == [300, 300, 300]
}

test "one person pays it all" {
  assert shares(1234, 1) == [1234]
}

test "everyone pays, and it adds up" {
  s := shares(1000, 4)
  assert len(s) == 4
  assert sum(s) == 1000
}
`);
    const src = run(dir, ['view', 'main']).out;
    const tests = defs(src, ['test ']);
    const t = out(dir, ['test']);
    const withContract = `/// Split a bill between people. The shares add up to the total.
fn shares(total: Cents, people: Positive) -> List[Cents]
  ensures sum(it) == total {
  [total / people for _ in 0..people]
}
`;
    const e1 = out(dir, ['edit', '-m', 'shares: say what must hold'], withContract);
    const v1 = out(dir, ['verify', 'shares']);
    const fixed = `/// Split a bill between people. The shares add up to the total.
fn shares(total: Cents, people: Positive) -> List[Cents]
  ensures sum(it) == total {
  extra := total % people
  [total / people + (if i < extra { 1 } else { 0 }) for i in 0..people]
}
`;
    out(dir, ['edit', '-m', 'shares: spread the remainder'], fixed);
    const v2 = out(dir, ['verify', 'shares']);
    V.shares = { code: defs(src, ['fn shares']), tests, test: t, contract: run(dir, ['view', 'shares', '--level', '2']).out.trim() && withContract.trim(), verify: v1, fixed: fixed.trim(), verify_fixed: v2 };
    if (e1.code) throw new Error('shares contract: ' + e1.out);
  }
  // four tests, or one line
  {
    const dir = prog('points', `/// Loyalty points earned on an order: 1 per euro, at most 500.
fn points(cents: Int where it >= 0) -> Int {
  min(cents / 100, 500)
}

test "no points under a euro" {
  assert points(99) == 0
}

test "one point per euro" {
  assert points(1234) == 12
}

test "capped at 500" {
  assert points(1000000) == 500
}

test "never negative" {
  assert points(0) >= 0
}
`);
    const src = run(dir, ['view', 'main']).out;
    const t = out(dir, ['test']);
    const one = `/// Loyalty points earned on an order: 1 per euro, at most 500.
fn points(cents: Int where it >= 0) -> Int
  ensures 0 <= it && it <= 500 && it <= cents / 100 {
  min(cents / 100, 500)
}
`;
    out(dir, ['edit', '-m', 'points: say it once'], one);
    V.points = { tests: src.trim(), test: t, one: one.trim(), verify: out(dir, ['verify', 'points']) };
  }
  // text in, a tree stored, text out
  {
    const dir = pkg('edits', [join(EX, 'orders.strata')]);
    const sloppy = 'fn line_total(l:Line)->Cents ensures it==l.qty*l.unit_price {l.unit_price*l.qty}';
    const bad = 'fn line_total(l: Line) -> Cents {\n  l.unit_price * l.sku\n}';
    V.edit = {
      sloppy, stored: { ...out(dir, ['edit', '-m', 'line_total: say what it is'], sloppy + '\n'), cmd: 'strata edit -m "line_total: say what it is"' },
      rendered: run(dir, ['view', 'line_total', '--level', '2']).out.trim(),
      bad, refused: { ...out(dir, ['edit'], bad + '\n'), cmd: 'strata edit' },
    };
  }
  // comprehensions and friends
  {
    const dir = prog('lines', `type Line = { sku: String, qty: Int, price: Int }

fn main() {
  lines := [
    Line { sku: "tea", qty: 2, price: 450 },
    Line { sku: "cup", qty: 1, price: 1200 },
    Line { sku: "pot", qty: 0, price: 3000 },
  ]
  ordered := [l.sku for l in lines where l.qty > 0]
  totals := Dict { l.sku: l.qty * l.price for l in lines }
  best := lines.sorted_by(l -> -l.qty * l.price)[0]
  print(f"ordered: {ordered}")
  print(f"totals: {totals}")
  print(f"biggest line: {best.sku}, {float(best.qty * best.price) / 100.0:.2}")
}
`);
    V.sugar = { src: run(dir, ['view', 'main']).out.trim(), run: out(dir, ['run']) };
  }
  // loops and termination, on examples/math.strata
  {
    const dir = pkg('v-math', [join(EX, 'math.strata')]);
    // (`--proofs`: views hide loop invariants by default)
    const src = run(dir, ['view', 'main', '--proofs']).out;
    V.loops = {};
    for (const [k, f] of [['ackermann', 'ackermann'], ['isqrt', 'isqrt'], ['triangle', 'triangle_closed'], ['collatz', 'collatz_steps']])
      V.loops[k] = { code: defs(src, [`fn ${f}(`]), verify: out(dir, ['verify', f, '--checks']) };
  }
  // contracts on behaviour: trait models, laws, generic code, a broken backend
  {
    /** A trait's members as rendered by `view --level 1`: its header (up to the first doc
     *  comment of a member), then each member with the doc comment above it. */
    const members = text => {
      const lines = text.split('\n'), open = lines.findIndex(l => /^[^\s/].*\{$/.test(l)), close = lines.lastIndexOf('}');
      const blocks = [];
      for (let i = open + 1; i < close; i++) {
        if (!blocks.length || (/^  (\/\/\/|fn |law |model |ensures |invariant )/.test(lines[i]) && !/^  \/\/\//.test(lines[i - 1]))) blocks.push([]);
        blocks.at(-1).push(lines[i]);
      }
      const isMember = b => b.some(l => /^  (fn|law) /.test(l));
      const head = [...lines.slice(0, open + 1), ...blocks.filter(b => !isMember(b)).flat()].join('\n');
      const parts = blocks.filter(isMember).map(b => b.join('\n'));
      return { head, parts, pick: re => parts.filter(p => re.test(p)) };
    };
    const C = {};
    const bank = pkg('v-bank', [join(EX, 'bank.strata')]);
    const ledger = members(run(bank, ['view', 'Ledger', '--level', '1']).out.trimEnd());
    C.model = [ledger.head, ...ledger.pick(/fn withdraw/), '  …', '}'].join('\n');
    C.laws = [ledger.head.split('\n').find(l => /^trait /.test(l)), '  …', ...ledger.pick(/^  law |\n  law /m), '}'].join('\n');
    C.sweep = defs(run(bank, ['view', '--level', '2']).out, ['fn sweep']);
    C.bank_verify = out(bank, ['verify', '--no-race']);
    // a store that keeps only the last order it saved: the trait's contract refutes it
    const orders = pkg('v-db', [join(EX, 'orders.strata')]);
    const db = run(orders, ['view', '--level', '2']).out;
    C.db = defs(db, ['trait Db']);
    const lossy = `impl Db for MemDb {
  model orders = self.all

  fn load(self, id: OrderId) -> Option[Order] {
    self.all.get(id)
  }

  /// Keeps only the order just saved.
  fn save(mut self, id: OrderId, o: Order) {
    self.all = Dict[OrderId, Order] {}
    mut self.all.insert(id, o)
  }
}
`;
    const e = out(orders, ['edit', '-m', 'MemDb: keep only the last order'], lossy);
    if (e.code) throw new Error('lossy MemDb: ' + e.out);
    C.lossy = lossy.trim();
    const all = out(orders, ['verify', '--no-race']);
    const keep = all.out.split('\n'), at = keep.findIndex(l => /^\S+ MemDb\.save /.test(l));
    const v = { ...all, cmd: all.cmd, out: [keep[at], ...keep.slice(at + 1).filter((l, i, a) => a.slice(0, i + 1).every(x => /^\s/.test(x)))].join('\n') };
    // the counterexample is a dictionary of random orders: keep the verdict and the contract it breaks
    C.lossy_verify = { ...v, out: v.out.split('\n').filter(l => !/^\s+input:/.test(l)).map(l => l.replace(/ \(before: .*$/, '').replace(/ \(stack: .*$/, '')).join('\n') };
    // a real queue: its laws, proven or fuzzed
    const tasks = join(WORK, 'v-tasks');
    cpSync(join(EX, 'tasks'), tasks, { recursive: true });
    const queue = members(run(tasks, ['view', 'model.Queue', '--level', '1']).out.trimEnd());
    C.queue = [queue.head, ...queue.pick(/fn claim\(/), '  …', ...queue.pick(/^  law /m), '}'].join('\n');
    const qv = out(tasks, ['verify', 'memory.*', '--no-race']);
    C.queue_verify = { ...qv, out: qv.out.split('\n').filter(l => /^\S+ law /.test(l)).join('\n') };
    // written proofs: a lemma, proven by induction, used where the solver alone can't
    const proofs = prog('proofs', readFileSync(join(ROOT, 'tests/cases/cli-lemmas_are_proven_once_and_used_in_proofs/trees.st'), 'utf8'));
    C.proofs = defs(run(proofs, ['view', '--level', '2', '--proofs']).out, ['type Tree', 'fn sum(', 'lemma sum_natural', 'fn total_plain', 'fn total(']);
    C.proofs_verify = out(proofs, ['verify', '--no-race']);
    V.behaviour = C;
  }
  // more of the language: data types, traits, values, comptime
  {
    const L = {};
    const one = (name, text) => { const dir = prog(name, text); return { dir, src: run(dir, ['view', 'main']).out.trim(), run: out(dir, ['run']) }; };
    const refused = (dir, text) => ({ ...out(dir, ['edit'], text), cmd: 'strata edit', input: text.trim() });
    const a = one('shapes', `/// A shape: one of three variants, each with its own fields.
type Shape =
  | Circle { radius: Positive }
  | Rect { w: Positive, h: Positive }
  | Point

type Percent = Int
  where 0 <= it && it <= 100

fn area(s: Shape) -> Natural {
  match s {
    Circle { radius } => 3 * radius * radius,
    Rect { w, h } => w * h,
    Point => 0,
  }
}

fn grade(score: Percent) -> String {
  match score {
    where it >= 90 => "A",
    where it >= 70 => "B",
    _ => "C",
  }
}

/// A percentage from text, or the reason it isn't one.
fn percent(text: String) -> Result[Percent, String] {
  n := parse_int(text) or { return Err(f"not a number: {text}") }
  if 0 <= n && n <= 100 {
    Ok(n)
  } else {
    Err(f"out of range: {n}")
  }
}

fn main() {
  print([area(s) for s in [Circle { radius: 2 }, Rect { w: 3, h: 4 }, Point]])
  print(grade(percent("93") !! 0))
  print(percent("150"))
  print(percent("ten"))
}
`);
    L.adt = { src: a.src, run: a.run, refused: refused(a.dir, `fn area(s: Shape) -> Natural {
  match s {
    Circle { radius } => 3 * radius * radius,
    Rect { w, h } => w * h,
  }
}
`) };
    const t = one('priced', `/// Anything with a price, in cents.
trait Priced {
  fn price(self) -> Natural

  /// A default method: every implementation gets it.
  fn label(self) -> String {
    f"{self.price() / 100}.{self.price() % 100:02}"
  }
}

type Tea = { grams: Positive }

type Cup = { color: String }

impl Priced for Tea {
  fn price(self) -> Natural {
    self.grams * 5
  }
}

impl Priced for Cup {
  fn price(self) -> Natural {
    1200
  }
}

/// Any type that implements \`Priced\`.
fn receipt(item: Priced) -> String {
  f"{item.label()} EUR"
}

fn main() {
  print(receipt(Tea { grams: 250 }))
  print(receipt(Cup { color: "blue" }))
  let basket: List[Priced] = [Tea { grams: 100 }, Cup { color: "red" }]
  print(sum([x.price() for x in basket]))
}
`);
    L.traits = { src: t.src, run: t.run };
    const m = one('cart', `type Cart = { items: List[String] }

/// Changes the caller's cart: the parameter says \`mut\`, and so does every call.
fn add(mut cart: Cart, item: String) {
  mut cart.items.push(item)
}

fn main() {
  a := Cart { items: ["tea"] }
  mut b := a
  add(mut b, "cup")
  print(a.items)
  print(b.items)
}
`);
    L.values = { src: m.src, run: m.run, refused: refused(m.dir, `fn main() {
  mut b := Cart { items: [] }
  add(b, "pot")
  print(b.items)
}
`) };
    const c = one('tables', `/// Primes below 60, found while compiling: the program only holds the list.
fn small_primes() -> List[Int] {
  comptime [n for n in 2..60 where all(2..n, d -> n % d != 0)]
}

/// A table of the squares of 0 to 12, built while compiling.
fn squares() -> List[Int] {
  comptime [n * n for n in 0..=12]
}

fn main() {
  print(small_primes())
  print(squares()[7])
}
`);
    L.comptime = { src: c.src, run: c.run };
    // concurrency: tasks, channels and select, race and supervised tasks
    const tk = one('primes', `/// How many primes there are from \`lo\` up to \`hi\`, by trial division.
fn count_primes(lo: Int, hi: Int) -> Int {
  len([n for n in lo..hi where n >= 2 && all(2..n, d -> n % d != 0)])
}

/// The primes below \`n\` and from \`n\` to twice \`n\`, a task for each.
fn halves(n: Int) -> (Int, Int) {
  scope {
    below := spawn count_primes(0, n)
    above := spawn count_primes(n, 2 * n)
    await (below, above)
  }
}

/// The primes below \`n\`, counted in four tasks, a quarter each.
fn primes_below(n: Int) -> Int {
  scope {
    sum(await [spawn count_primes(i * n / 4, (i + 1) * n / 4) for i in 0..4])
  }
}

fn main() {
  print(halves(1000))
  print(primes_below(2000))
}
`);
    const ch = one('progress', `/// The primes below \`n\`, counted by a task that reports its progress.
fn with_progress(n: Int) -> (Int, List[Int]) {
  (tx, rx) := channel()
  scope {
    work := spawn {
      mut found := 0
      for c in 0..n / 500 {
        found += len([k for k in c * 500..(c + 1) * 500 where k >= 2 && all(2..k, d -> k % d != 0)])
        tx.send((c + 1) * 500)
      }
      tx.close()
      found
    }
    tx.close()
    mut reports := []
    mut primes := None
    while primes == None {
      select {
        rx.recv() as r => match r {
          Some(k) => {
            mut reports.push(k)
          },
          None => {},
        },
        await work as found => {
          primes = Some(found)
        },
      }
    }
    (primes ?? 0, reports)
  }
}

fn main() {
  print(with_progress(2000))
}
`);
    const rc = one('readings', `from std.time use Clock, VirtualClock, seconds

/// The first reading, or \`None\` after 30 seconds. The loser is cancelled.
fn first_reading(readings: Receiver[Int], clock: Clock) -> Option[Int] {
  race {
    readings.recv() as r => r,
    clock.sleep(seconds(30)) => None,
  }
}

/// The average of each batch, each in a task of its own. When one fails,
/// the others go on, and its panic is handled where it is awaited.
fn averages(batches: List[List[Int]]) -> List[String] {
  scope {
    tasks := [supervised spawn sum(b) / len(b) for b in batches]
    [match await h {
      Ok(a) => f"{a}",
      Err(p) => p.message,
    } for h in tasks]
  }
}

fn main(clock: VirtualClock) {
  (tx, rx) := channel()
  print(first_reading(rx, clock))
  tx.close()
  print(clock.now().value / 1000000000)
  print(averages([[4, 6], [], [1, 2, 3]]))
}
`);
    L.tasks = { spawn: tk, channels: ch, race: rc };
    V.lang = L;
  }
  emit('verify', V);
}

rmSync(WORK, { recursive: true, force: true });
