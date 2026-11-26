// Records what the landing page's comparisons show (site/data/landing.js):
// the ledger in Strata (`strata verify`) and in Rust (`cargo test`), and the
// order system's session redone in Rust (the same edits, with `git diff`,
// `cargo build` and `cargo test`). The Strata side of the session is the
// tour's, recorded by build.mjs into data/session.js.
//
//   STRATA=path/to/strata node site/landing/record.mjs   (needs cargo)

import { spawnSync } from 'node:child_process';
import { mkdtempSync, realpathSync, readFileSync, writeFileSync, cpSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const SITE = dirname(HERE);
const STRATA = process.env.STRATA || 'strata';
const WORK = realpathSync(mkdtempSync(join(tmpdir(), 'strata-landing-')));

/** Run a command in `cwd`: its output as a terminal shows it. */
function sh(cwd, cmd, args, input) {
  const r = spawnSync(cmd, args, { cwd, input, encoding: 'utf8', env: { ...process.env, NO_COLOR: '1', CARGO_TERM_COLOR: 'never' } });
  return { out: (r.stdout + r.stderr).replaceAll(WORK, '~').replace(/\s+$/, ''), code: r.status };
}
const git = (cwd, ...a) => sh(cwd, 'git', a).out;
/** `cargo` output without the progress lines and the warnings, which are noise here. */
function cargo(cwd, ...a) {
  const r = sh(cwd, 'cargo', a);
  const lines = r.out.split('\n').filter(l => !/^\s*(Compiling|Finished|Running|Doc-tests|Blocking)\b/.test(l));
  const blocks = lines.join('\n').split(/\n(?=\S)/).filter(b => !/^warning/.test(b));
  return { cmd: `cargo ${a.join(' ')}`, out: blocks.join('\n').replace(/\n{3,}/g, '\n\n').trim(), code: r.code };
}
const diffCount = d => d.split('\n').filter(l => /^[+-]/.test(l) && !/^(\+\+\+|---)/.test(l)).length;

// ───────── the ledger ─────────
const ledgerDir = join(WORK, 'ledger');
cpSync(HERE, ledgerDir, { recursive: true, filter: s => !s.includes('/target') });
sh(ledgerDir, STRATA, ['--file', 'ledger.strata', 'edit'], readFileSync(join(HERE, 'ledger.st'), 'utf8'));
const ledger = {
  strata: sh(ledgerDir, STRATA, ['--file', 'ledger.strata', 'view', 'main']).out,
  verify: { cmd: 'strata verify', ...sh(ledgerDir, STRATA, ['--file', 'ledger.strata', 'verify']) },
  rust: readFileSync(join(HERE, 'ledger-rs/src/lib.rs'), 'utf8').trimEnd(),
  test: cargo(join(ledgerDir, 'ledger-rs'), 'test', '--lib'),
};
ledger.verify.out = ledger.verify.out.replaceAll(' (cached)', '');

// ───────── the order system, redone in Rust: the tour session's edits ─────────
const shop = join(WORK, 'shop');
cpSync(join(HERE, 'shop-rs'), shop, { recursive: true, filter: s => !s.includes('/target') });
git(shop, 'init', '-q'); git(shop, 'add', '.'); git(shop, '-c', 'user.name=agent', '-c', 'user.email=agent@example.com', 'commit', '-qm', 'orders');
const commit = m => { git(shop, 'add', '.'); git(shop, '-c', 'user.name=agent', '-c', 'user.email=agent@example.com', 'commit', '-qm', m); };
const src = f => readFileSync(join(shop, 'src', f), 'utf8');
const put = (f, s) => writeFileSync(join(shop, 'src', f), s);
const edit = (f, from, to) => { const s = src(f); if (!s.includes(from)) throw new Error(`${f}: no ${from}`); put(f, s.replace(from, to)); };
const files = readdirSync(join(shop, 'src')).filter(f => f.endsWith('.rs')).sort();
cargo(shop, 'build'); // (so the steps' outputs don't include the first build)

const steps = {};
// 1. finding its way: the files, all of them
steps.read = { cmd: 'cat src/*.rs', text: files.map(f => `// src/${f}\n${src(f).trimEnd()}`).join('\n\n') };

// 2. the unfinished function: todo!() compiles, and says nothing about what is missing
steps.fill = {
  find: { cmd: 'grep -rn "todo!" src', out: sh(shop, 'grep', ['-rn', 'todo!', 'src']).out },
};
edit('orders.rs', `/// What to refund for an order. Never negative.
pub fn refund(o: &Order) -> Cents {
    todo!()
}`, `/// What to refund for an order: what was paid, minus a 5.00 handling fee.
/// Never negative.
pub fn refund(o: &Order) -> Cents {
    match o {
        Order::Pending { lines, .. } => total(lines) - 500,
        Order::Shipped { .. } => 0,
        Order::Cancelled { .. } => 0,
    }
}`);
edit('orders.rs', `        assert_eq!(discount(20000), 18000);
    }
}`, `        assert_eq!(discount(20000), 18000);
    }

    #[test]
    fn refund_keeps_the_handling_fee() {
        let o = Order::Pending { id: "ord_abc123".into(), lines: vec![line("tea", 2, 450)] };
        assert_eq!(refund(&o), 400);
    }
}`);
steps.fill.diff = git(shop, 'diff');
// 3. the tests pass: the order worth less than the fee isn't among them
steps.fill.test = cargo(shop, 'test', '--lib');
commit('refund policy');

// 4. a new parameter: the build breaks where `total` is called
edit('orders.rs', `pub fn total(lines: &[Line]) -> Cents {
    lines.iter().map(line_total).sum()
}`, `pub fn total(lines: &[Line], coupon: Cents) -> Cents {
    (lines.iter().map(line_total).sum::<Cents>() - coupon).max(0)
}`);
steps.coupons = { build: cargo(shop, 'build', '--all-targets') };
edit('orders.rs', 'Order::Pending { lines, .. } => total(lines) - 500,', 'Order::Pending { lines, .. } => total(lines, 0) - 500,');
edit('orders.rs', 'assert_eq!(total(&lines), 350);', 'assert_eq!(total(&lines, 0), 350);\n        assert_eq!(total(&lines, 100), 250);');
steps.coupons.diff = git(shop, 'diff');
steps.coupons.test = cargo(shop, 'test', '--lib');
commit('coupons');

// 5. a rename: every file that mentions it
for (const f of files) put(f, src(f).replaceAll('Shipped', 'Sent'));
steps.rename = { cmd: "sed -i '' 's/Shipped/Sent/g' src/*.rs", diff: git(shop, 'diff'), build: cargo(shop, 'build') };
commit('rename Shipped to Sent');

for (const s of [steps.fill, steps.coupons, steps.rename]) s.lines = diffCount(s.diff);
steps.read.lines = steps.read.text.split('\n').length;

writeFileSync(join(SITE, 'data/landing.js'), `// generated by site/landing/record.mjs: do not edit\nSTRATA["landing"] = ${JSON.stringify({ ledger, shop: steps })};\n`);
console.log('wrote site/data/landing.js');
