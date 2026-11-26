// The landing page: the install line, the way into the tour, and a few
// functions from examples/math.strata with what `strata verify` said about them
// (recorded by build.mjs into data/math.js).
'use strict';

// The command that installs Strata (site/install.sh).
const INSTALL = 'curl -fsSL https://strata-lang.dev/install.sh | sh';

$('#install-cmd').innerHTML = `<span class="prompt">$</span> ${esc(INSTALL)}`;
$('#copy').addEventListener('click', async () => {
  const b = $('#copy');
  try { await navigator.clipboard.writeText(INSTALL); b.textContent = 'Copied'; } catch { b.textContent = 'Select it'; }
  b.classList.add('done'); setTimeout(() => { b.textContent = 'Copy'; b.classList.remove('done'); }, 1600);
});

// each letter a shade further along, from white through violet to amber, like the bands
const SHADES = ['#ecebe6', '#dcd6f2', '#c9bdf7', '#c3a6e6', '#d9a98f', '#f0b45c'];
$('#title').innerHTML = [...'Strata'].map((c, i) => `<span class="l" style="color:${SHADES[i]};animation-delay:${0.1 + i * 0.07}s">${c}</span>`).join('');

// ───────── the layered bands behind the page ─────────
(() => {
  const svg = $('#bands');
  const colors = ['#1a1d2b', '#1d2033', '#22233a', '#2a2540', '#3a2c3c', '#4d3438', '#5e3f33'];
  const W = 1600, paths = colors.map((c, i) => {
    const p = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    p.setAttribute('fill', c); p.setAttribute('opacity', 0.5 + i * 0.05); svg.appendChild(p); return p;
  });
  let t = 0, scroll = 0, s = 0;
  addEventListener('scroll', () => { scroll = scrollY; }, { passive: true });
  (function frame() {
    t += 0.0025; s += (scroll - s) * 0.06;
    paths.forEach((p, i) => {
      const base = 610 + i * 62 - s * (0.05 + i * 0.03);
      let d = `M0 ${base}`;
      for (let x = 0; x <= W; x += 80) {
        const y = base + Math.sin(x / 260 + t * (1 + i * 0.3) + i) * (18 + i * 4) + Math.sin(x / 90 + t * 2 + i * 2) * 4;
        d += ` L${x} ${y.toFixed(1)}`;
      }
      p.setAttribute('d', d + ` L${W} 1000 L0 1000 Z`);
    });
    requestAnimationFrame(frame);
  })();
})();

// ───────── the showcase: code typed out, then the verdict ─────────
{
  const graph = STRATA.math.graph, verify = STRATA.math.verify.split('\n');
  /** A definition's text, without the comment banners above it. */
  const src = name => graph.find(g => g.name === name).text.split('\n').filter(l => !l.startsWith('//')).join('\n').trim();
  /** What verify said about `name`: its line and the indented ones under it. */
  const said = name => {
    const i = verify.findIndex(l => new RegExp(`^.\\s+${name}\\s`).test(l));
    let j = i + 1; while (j < verify.length && /^\s/.test(verify[j])) j++;
    return verify.slice(i, j);
  };
  const V = STRATA.verify;
  const fromMath = names => ({ code: names.map(src).join('\n\n'), lines: names.flatMap(said) });
  const shows = [
    { tab: 'A broken promise', ...fromMath(['Digit', 'next_digit']), badge: ['bad', 'refuted'],
      note: 'The solver finds the input that breaks the type, before anything runs.' },
    { tab: 'Proven', ...fromMath(['is_prime', 'primes']), badge: ['ok', 'proven'],
      note: 'The promise in each signature holds for every input.' },
    { tab: 'A crash', ...fromMath(['average']), badge: ['bad', 'refuted'],
      note: 'A division by zero, found without a single test.' },
    { tab: 'May not end', ...fromMath(['collatz_steps']), badge: ['warn', 'termination unproven'],
      note: 'The contract is proven, but nobody knows whether this loop always ends. verify says so instead of guessing.' },
  ];
  $('#tabs').innerHTML = shows.map((s, i) => `<button data-i="${i}">${s.tab}</button>`).join('');
  let run = 0, cur = 0;
  async function show(i, typing = true) {
    const me = ++run; cur = i;
    $$('#tabs button').forEach((b, j) => b.classList.toggle('on', j === i));
    const s = shows[i], code = s.code, lines = s.lines;
    $('#file').textContent = s.file || 'math.strata';
    const body = $('#code'), out = $('#out'), badge = $('#badge');
    out.innerHTML = ''; badge.className = 'count'; badge.textContent = '';
    if (typing) {
      for (let k = 0; k <= code.length; k += 2) {
        if (me !== run) return;
        body.innerHTML = hl(code.slice(0, k)) + '<span class="caret">▍</span>';
        await sleep(code[k] === '\n' ? 40 : 14);
      }
    }
    body.innerHTML = hl(code);
    await sleep(typing ? 450 : 0); if (me !== run) return;
    for (const l of lines) {
      out.insertAdjacentHTML('beforeend', `<span class="term-line in">${termLine(l)}</span>`);
      await sleep(typing ? 220 : 0); if (me !== run) return;
    }
    out.insertAdjacentHTML('beforeend', `<span class="term-line note in">${esc(s.note)}</span>`);
    badge.className = `count ${s.badge[0]}`; badge.textContent = s.badge[1];
    if (!typing) return;
    await sleep(7500); if (me !== run) return;
    show((i + 1) % shows.length);
  }
  $('#tabs').addEventListener('click', e => { const b = e.target.closest('button'); if (b) show(+b.dataset.i); });
  // with reduced motion: no typing, no rotation
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  if (still) show(0, false); else setTimeout(() => show(0), 900);
}

// ───────── what Strata changes for agents: the same programs in Rust and in Strata ─────────
// Everything here is recorded: the Rust side by site/landing/record.mjs
// (data/landing.js), the Strata side by build.mjs (data/session.js, data/orders.js).

/** Rust, coloured like Strata. */
const RUST_KW = new Set(('fn pub struct enum impl trait let mut match if else return use mod for in self Self crate super where ' +
  'as ref move loop while break continue const static type dyn true false').split(' '));
function hlRust(code) {
  const re = /(\/\/[^\n]*)|("(?:[^"\\]|\\.)*")|(#\[[^\]]*\])|(\b\d[\d_]*\b)|([A-Za-z_][A-Za-z0-9_]*!)|([A-Za-z_][A-Za-z0-9_]*)(?=\s*(?:\(|::<))|([A-Za-z_][A-Za-z0-9_]*)|([\s\S])/g;
  let out = '', m;
  while ((m = re.exec(code))) {
    if (m[1]) out += `<span class="${m[1].startsWith('///') ? 'c doc' : 'c'}">${esc(m[1])}</span>`;
    else if (m[2]) out += `<span class="s">${esc(m[2])}</span>`;
    else if (m[3]) out += `<span class="c">${esc(m[3])}</span>`;
    else if (m[4]) out += `<span class="nm">${m[4]}</span>`;
    else if (m[5]) out += `<span class="f">${esc(m[5])}</span>`;
    else if (m[6]) out += RUST_KW.has(m[6]) ? `<span class="k">${m[6]}</span>` : /^[A-Z]/.test(m[6]) ? `<span class="t">${m[6]}</span>` : `<span class="f">${m[6]}</span>`;
    else if (m[7]) out += RUST_KW.has(m[7]) ? `<span class="k">${m[7]}</span>` : /^[A-Z]/.test(m[7]) ? `<span class="t">${m[7]}</span>` : esc(m[7]);
    else out += esc(m[8]);
  }
  return out;
}
/** A git diff of Rust code: added and removed lines, each highlighted. */
const rustDiff = d => d.split('\n').filter(l => !/^(diff --git|index )/.test(l)).map(l => {
  const c = /^(\+\+\+|---)/.test(l) ? 'd-file' : l[0] === '+' ? 'd-add' : l[0] === '-' ? 'd-del' : l[0] === '@' ? 'd-hunk' : 'd-ctx';
  return c === 'd-file' || c === 'd-hunk' ? `<span class="${c}">${esc(l)}</span>` : `<span class="${c}">${esc(l[0] || ' ')}${hlRust(l.slice(1)) || ' '}</span>`;
}).join('');

/** What strata printed, with the code in it coloured: the map of `view --level 0`
 *  (kind, name, what it uses), renames, and a hole's goal and context. Other lines as `term` colours them. */
function strataOut(out) {
  const name = (n, kind) => /^impl /.test(n) ? hl(n) : `<span class="${kind === 'fn' || kind === 'test' ? 'f' : 't'}">${esc(n)}</span>`;
  const names = list => list.split(', ').map(n => /^[A-Z]/.test(n) ? `<span class="t">${esc(n)}</span>` : `<span class="f">${esc(n)}</span>`).join('<span class="o-dim">, </span>');
  return out.split('\n').map(l => {
    let m;
    // a line of the map: `fn     ship  → Db, Order  // [holes: ?x]`
    if ((m = l.match(/^(trait|type|fn|impl|test)(\s+)(.+?)(?:(\s+→ )(.+?))?(\s+\/\/ .*)?$/)) && !/\b(changed|added|removed)\b/.test(l))
      return `<span class="term-line"><span class="k">${m[1]}</span>${m[2]}${name(m[3], m[1])}${m[4] ? `<span class="o-dim">${m[4]}</span>${names(m[5])}` : ''}${m[6] ? `<span class="c">${esc(m[6]).replace(/(\?[a-z_0-9]+)/, '<span class="h">$1</span>')}</span>` : ''}</span>`;
    if ((m = l.match(/^renamed (\S+) → (\S+)$/)))
      return `<span class="term-line">renamed <span class="o-bad">${esc(m[1])}</span> <span class="o-dim">→</span> <span class="o-ok">${esc(m[2])}</span></span>`;
    if ((m = l.match(/^(updated \d+ definitions? that refer to it: )(.*)$/)))
      return `<span class="term-line">${esc(m[1])}${names(m[2])}</span>`;
    if ((m = l.match(/^(\s+)(goal|context|was):(\s+)(.*)$/)))
      return `<span class="term-line">${m[1]}<span class="o-dim">${m[2]}:</span>${m[3]}${hl(m[4])}</span>`;
    return `<span class="term-line">${termLine(l)}</span>`;
  }).join('');
}

{
  const D = STRATA, L = D.landing, S = D.session;
  const pane = (title, body, { cls = '' } = {}) =>
    `<div class="pane ${cls}"><div class="bar"><span class="dots"><i></i><i></i><i></i></span><span class="title">${esc(title)}</span></div><div class="body${cls.includes('term') ? ' wrap' : ''}">${body}</div></div>`;
  const run = (cmd, out) => cmdLine(cmd) + (out ? term(out) : '');
  const side = (cls, label, n, html) => `<div class="side ${cls}"><h4>${esc(label)} <span class="big-n">${esc(n)}</span></h4>${html}</div>`;

  // ── contracts: the ledger ──
  // the Strata ledger as `strata view` printed it, in reading order: types, the trait (one law in full), the implementation's transfer
  const defs = L.ledger.strata.split(/\n\n(?=\S)/);
  const def = re => defs.find(d => re.test(d));
  const ledger = [def(/type Cents/), def(/type Account/), def(/^(\/\/\/.*\n)*trait Ledger/), 'impl Ledger for MemLedger { … }'].join('\n\n');
  // the Rust file with its implementation folded the same way
  const rust = L.ledger.rust.replace(/impl Ledger for MemLedger \{[\s\S]*?\n\}\n/, 'impl Ledger for MemLedger { … }\n');
  const V = L.ledger.verify;
  $('#ch-ledger').innerHTML = `<div class="vs">` +
    side('old', 'Rust: rules in comments', '3 tests pass', pane('ledger.rs', hlRust(rust), { cls: 'rust' }) + pane('~/ledger', run(L.ledger.test.cmd, L.ledger.test.out), { cls: 'term rust' })) +
    side('new', 'Strata: rules in code', '1 bug found', pane('ledger.strata', hl(ledger)) + pane('~/ledger', run(V.cmd, V.out), { cls: 'term' })) + `</div>`;

  // ── editing the syntax tree: the tour's session, next to the same edits in Rust ──
  const R = L.shop, at = t => S.find(s => s.title === t);
  const holes = at('The agent asks what is left to do'), fill = at('It fills the hole'), bug = at('Verification finds the bug'), fix = at('It fixes it, and proves it');
  const broke = at('A breaking change becomes holes'), closed = at('It fills them, and closes the change'), renamed = at('A rename is one change');
  const every = [...new Map(S.flatMap(s => s.graph).map(d => [d.name, d])).values()];
  const strataRun = s => cmdLine(s.cmd) + (s.input ? piped(s.input.trim()) : '') + strataOut(s.out);
  const steps = [
    { tab: 'Read it', say: 'To find its way, an agent working in Rust reads the files. In Strata it asks for a view of the part it needs, at the level of detail it needs.',
      rust: [`${R.read.lines} lines`, pane('cat src/*.rs', hlRust(R.read.text), { cls: 'rust' })],
      strata: ['9 lines', pane('~/shop', cmdLine('strata view slice:Order --level 0') + strataOut(D.orders.levels[0].trimEnd()), { cls: 'term' })],
      graph: g => g.set(S[0].graph, { layout: every, focus: 'Order' }) },
    { tab: 'Fill a gap', say: 'A Rust <code>todo!()</code> compiles and says nothing about what is missing. A Strata hole is a task: the type to produce, what is in scope, the contract to meet. The edit that fills it is checked before it is stored.',
      rust: [`${R.fill.lines} lines changed`, pane('~/shop', run(R.fill.find.cmd, R.fill.find.out), { cls: 'term rust' }) + pane('git diff', rustDiff(R.fill.diff), { cls: 'rust' })],
      strata: ['1 hole filled', pane('~/shop', strataRun({ ...holes, out: D.orders.holes }) + '\n' + strataRun(fill), { cls: 'term' })],
      graph: g => g.set(fill.graph, { layout: every, focus: 'refund', changed: new Set(['refund', 'test "refund keeps the handling fee"']) }) },
    { tab: 'Check it', say: 'The Rust tests pass. Strata’s <code>verify</code> finds the order the test didn’t try: one worth less than the fee gets a negative refund. After the fix, <code>refund</code> is proven for every order.',
      rust: ['2 tests pass', pane('~/shop', run(R.fill.test.cmd, R.fill.test.out), { cls: 'term rust' })],
      strata: ['1 bug found, then proven', pane('~/shop', strataRun(bug) + '\n' + strataRun(fix), { cls: 'term' })],
      graph: g => g.set(fix.graph, { layout: every, focus: 'refund', changed: new Set(['refund']) }) },
    { tab: 'Change a signature', say: '<code>total</code> takes a coupon. In Rust the build stops at the calls that no longer fit, to be found and fixed by hand. In Strata they become holes in one pending change, each saying what it was and why it broke.',
      rust: [`${(R.coupons.build.out.match(/^error\[/gm) || []).length} errors`, pane('~/shop', run(R.coupons.build.cmd, R.coupons.build.out), { cls: 'term rust' }) + pane('git diff', rustDiff(R.coupons.diff), { cls: 'rust' })],
      strata: ['2 holes, filled', pane('~/shop', strataRun(broke) + '\n' + strataRun(closed), { cls: 'term' })],
      graph: g => g.set(closed.graph, { layout: every, changed: changedBetween(fix.graph, closed.graph) }) },
    { tab: 'Rename', say: 'Rust renames by search and replace, read back as a diff. Strata renames in the tree: names are resolved, so the change is one rename, and every use follows it.',
      rust: [`${R.rename.lines} lines changed`, pane('~/shop', run(R.rename.cmd, ''), { cls: 'term rust' }) + pane('git diff', rustDiff(R.rename.diff), { cls: 'rust' })],
      strata: ['1 change', pane('~/shop', strataRun(renamed), { cls: 'term' })],
      graph: g => g.set(renamed.graph, { layout: every, changed: changedBetween(closed.graph, renamed.graph) }) },
  ];
  const root = $('#ch-session'), tabs = $('.vtabs', root), graph = new Graph($('.graph', root), { legend: false });
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches, EVERY = 9000;
  tabs.innerHTML = steps.map((s, i) => `<button data-i="${i}"><b>${i + 1}</b> ${s.tab}<i class="fill"></i></button>`).join('');
  let cur = 0, timer = null, seen = false, held = false;
  const show = i => {
    cur = i; const s = steps[i];
    $$('button', tabs).forEach((b, j) => { b.classList.remove('on'); if (j === i) { void b.offsetWidth; b.classList.add('on'); } });
    root.classList.toggle('playing', seen && !held && !still);
    $('.caption', root).innerHTML = s.say;
    $('.old .big-n', root).textContent = s.rust[0]; $('.rust', root).innerHTML = s.rust[1];
    $('.new .big-n', root).textContent = s.strata[0]; $('.strata', root).innerHTML = s.strata[1];
    s.graph(graph);
  };
  // the steps go round on their own while the section is on screen, and stop while the pointer is over it
  const tick = () => { clearTimeout(timer); if (seen && !held && !still) timer = setTimeout(() => { show((cur + 1) % steps.length); tick(); }, EVERY); };
  new IntersectionObserver(es => { seen = es.some(e => e.isIntersecting); show(cur); tick(); }, { threshold: .3 }).observe(root);
  root.addEventListener('mouseenter', () => { held = true; root.classList.remove('playing'); tick(); });
  root.addEventListener('mouseleave', () => { held = false; show(cur); tick(); });
  tabs.addEventListener('click', e => { const b = e.target.closest('button'); if (b) { show(+b.dataset.i); tick(); } });
  root.style.setProperty('--every', EVERY + 'ms');
  show(0);
}
