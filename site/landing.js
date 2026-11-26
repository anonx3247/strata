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

// ───────── what Strata changes for agents: the usual way next to Strata's, from the same recordings ─────────
{
  const D = STRATA, V = D.verify;
  const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const pane = (title, body, { cls = '' } = {}) =>
    `<div class="pane ${cls}"><div class="bar"><span class="dots"><i></i><i></i><i></i></span><span class="title">${esc(title)}</span></div><div class="body${cls.includes('term') ? ' wrap' : ''}">${body}</div></div>`;
  const run = (cmd, out) => cmdLine(cmd) + term(out);
  /** The usual way on the left, Strata's on the right, each with a size or a result. */
  const compare = (usual, strata) => `<div class="vs">
    <div class="side old"><h4>${esc(usual.label)} <span class="big-n">${esc(usual.n)}</span></h4>${usual.html}</div>
    <div class="side new"><h4>${esc(strata.label)} <span class="big-n">${esc(strata.n)}</span></h4>${strata.html}</div>
  </div>`;
  /** Run `play` once, when `el` first scrolls into view. */
  const whenSeen = (el, play) => new IntersectionObserver((es, o) => { if (es.some(e => e.isIntersecting)) { o.disconnect(); play(); } }, { threshold: .35 }).observe(el);

  // tests miss inputs
  const H = V.shares;
  $('#ch-shares').innerHTML = compare(
    { label: 'Usual: tests', n: '3 tests pass', html: pane('shares.strata', hl(H.code + '\n\n' + H.tests)) + pane('~/shares', run(H.test.cmd, H.test.out), { cls: 'term' }) },
    { label: 'Strata: a contract', n: '1 line, a bug found', html: pane('shares.strata', hl(H.contract)) + pane('~/shares', run(H.verify.cmd, H.verify.out), { cls: 'term' }) });

  // rules that are checked: one example, a ledger, in Rust and in Strata
  const RUST = `/// Accounts holding money. Its rules, which nothing checks:
/// - a balance is never negative
/// - accounts are never removed
/// - a transfer keeps the total of the two balances
/// - a transfer that fails changes nothing
pub trait Ledger {
    /// Every account's balance, by owner.
    fn balances(&self) -> &HashMap<String, i64>;

    /// Takes money out, if there is enough; otherwise
    /// leaves the balance as it was.
    fn withdraw(&mut self, id: &str, amount: i64)
        -> Result<(), LedgerError>;

    /// Moves money from one account to another.
    fn transfer(&mut self, from: &str, to: &str, amount: i64)
        -> Result<(), LedgerError>;
}`;
  const B = V.behaviour, ledger = B.model.replace(/\n  …\n}\s*$/, '') + '\n\n' + B.laws.replace(/^trait Ledger \{\n  …\n/, '');
  const proven = B.bank_verify.out.split('\n').filter(l => l.startsWith('✓')).length;
  $('#ch-ledger').innerHTML = compare(
    { label: 'Usual: Rust', n: 'rules in comments', html: pane('ledger.rs', esc(RUST), { cls: 'plain' }) },
    { label: 'Strata', n: `${proven} proven`, html: pane('examples/bank.strata', hl(ledger)) + pane('~/bank', run(B.bank_verify.cmd, B.bank_verify.out), { cls: 'term' }) });

  // a graph of definitions: the code written out, each definition taking its place in the graph
  const s0 = D.session[0], g0 = s0.graph;
  $('#ch-graph').innerHTML = `<div class="astpair">${pane('orders.strata · rendered as text', '', { cls: 'code' })}<div class="graph"></div></div>`;
  {
    const graph = new Graph($('#ch-graph .graph'), { legend: false }), code = $('#ch-graph .body');
    const at = graph.layout(g0), order = [...g0].sort((a, b) => at.get(a.name).x - at.get(b.name).x || at.get(a.name).y - at.get(b.name).y);
    const all = () => { graph.set(g0, { statuses: false, fast: true }); code.innerHTML = order.map(d => hl(d.text)).join('\n\n'); };
    if (still) all();
    else {
      graph.set([], { layout: g0 });
      whenSeen($('#ch-graph'), async () => {
        const shown = [];
        for (const d of order) {
          const el = document.createElement('div'); code.appendChild(el);
          if (shown.length) el.style.marginTop = '1.6em';
          for (const l of d.text.split('\n')) { el.insertAdjacentHTML('beforeend', `<span class="ln">${hl(l) || ' '}</span>`); code.scrollTop = code.scrollHeight; await sleep(14); }
          shown.push(d);
          graph.set(shown, { layout: g0, statuses: false, changed: new Set([d.name]) });
          await sleep(60);
        }
        await sleep(400); code.scrollTop = 0;
      });
    }
  }

  // views
  const fix = D.session.find(s => s.title === 'It fixes it, and proves it');
  const files = fix.graph.map(d => d.text).join('\n\n'), map = D.orders.levels[0].trimEnd();
  $('#ch-views').innerHTML = compare(
    { label: 'Usual: read the files', n: `${files.split('\n').length} lines`, html: pane('the order system, every file', hl(files), { cls: 'long' }) },
    { label: 'Strata: a view', n: `${map.split('\n').length} lines`, html: pane('$ strata view slice:Order --level 0', term(map), { cls: 'term' }) });

  // typed holes: the graph next to the terminal, a step at a time
  const fill = D.session.find(s => s.title === 'It fills the hole'), bad = D.session.find(s => s.title === 'A broken edit is refused');
  const every = [...new Map(D.session.flatMap(s => s.graph).map(d => [d.name, d])).values()];
  const steps = [
    { tab: 'The task', term: run('strata holes', D.orders.holes), show: g => g.set(g0, { layout: every, focus: 'refund' }) },
    { tab: 'Filled, and checked', term: cmdLine(fill.cmd) + piped(fill.input.trim()) + term(fill.out),
      show: g => g.set(fill.graph, { layout: every, changed: new Set(['refund', 'test "refund keeps the handling fee"']), focus: 'refund' }) },
    { tab: 'A bad edit, refused', term: cmdLine(bad.cmd) + piped(bad.input.trim()) + term(bad.out), show: g => { g.set(fill.graph, { layout: every }); g.shake(); } },
  ];
  $('#ch-holes').innerHTML = `<div class="vtabs"></div><div class="astpair">${pane('~/shop', '', { cls: 'term' })}<div class="graph"></div></div>`;
  {
    const graph = new Graph($('#ch-holes .graph')), body = $('#ch-holes .pane .body'), tabs = $('#ch-holes .vtabs');
    tabs.innerHTML = steps.map((s, i) => `<button data-i="${i}">${s.tab}</button>`).join('');
    let cur = 0, timer = null;
    const show = i => {
      cur = i; $$('button', tabs).forEach((b, j) => b.classList.toggle('on', j === i));
      body.innerHTML = steps[i].term; steps[i].show(graph);
    };
    const auto = () => { clearInterval(timer); if (!still) timer = setInterval(() => show((cur + 1) % steps.length), 6000); };
    tabs.addEventListener('click', e => { const b = e.target.closest('button'); if (b) { show(+b.dataset.i); auto(); } });
    show(0);
    whenSeen($('#ch-holes'), auto);
  }

  // structural diffs
  const R = D.rename, st = structural(R.changes[R.step]), n = diffCount(R.diff);
  $('#ch-diff').innerHTML = compare(
    { label: 'Usual: git diff', n: `${n} lines`, html: pane('git diff', diffHtml(R.diff)) },
    { label: 'Strata: strata diff', n: `${st.count} change${st.count > 1 ? 's' : ''}`, html: pane('strata diff', st.html, { cls: 'term' }) });
}
