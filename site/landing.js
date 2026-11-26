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
    { tab: 'Tests passed', file: 'shares.strata', code: V.shares.contract, lines: V.shares.verify.out.split('\n'), badge: ['bad', 'refuted'],
      note: 'Three tests of this function passed. One line of contract, and verify finds the bill they missed: 58 cents between 13 people loses 6.' },
    { tab: 'A broken promise', ...fromMath(['Digit', 'next_digit']), badge: ['bad', 'refuted'],
      note: 'The solver finds the input that breaks the type, before anything runs.' },
    { tab: 'Proven', ...fromMath(['is_prime', 'primes']), badge: ['ok', 'proven'],
      note: 'The promise in each signature holds for every input.' },
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

// ───────── the tour, in short: one visual per main point, from the same recordings ─────────
{
  const D = STRATA, V = D.verify;
  const pane = (title, body, { cls = '', count = '' } = {}) =>
    `<div class="pane ${cls}"><div class="bar"><span class="dots"><i></i><i></i><i></i></span><span class="title">${esc(title)}</span>${count}</div><div class="body${cls.includes('term') ? ' wrap' : ''}">${body}</div></div>`;
  const run = (cmd, out) => cmdLine(cmd) + term(out);

  $('#ch-contracts').innerHTML =
    pane('contracts.strata', hl([V.contracts.types, V.contracts.discount, V.contracts.callers].join('\n\n'))) +
    pane('~/contracts', run(V.contracts.verify.cmd, V.contracts.verify.out), { cls: 'term' });

  $('#ch-laws').innerHTML =
    pane('examples/bank.strata', hl(V.behaviour.laws)) +
    pane('~/bank', run(V.behaviour.bank_verify.cmd, V.behaviour.bank_verify.out), { cls: 'term' });

  const P = V.points;
  $('#ch-points').innerHTML =
    `<div><div class="cap bad">four tests</div>${pane('points.strata', hl(P.tests))}${pane('~/points', run(P.test.cmd, P.test.out), { cls: 'term' })}</div>` +
    `<div><div class="cap ok">one line</div>${pane('points.strata', hl(P.one))}${pane('~/points', run(P.verify.cmd, P.verify.out), { cls: 'term' })}</div>`;

  const s0 = D.session[0], fill = D.session.find(s => s.title === 'It fills the hole'), bad = D.session.find(s => s.title === 'A broken edit is refused');
  const refund = s0.graph.find(d => d.name === 'refund').text;
  $('#ch-holes').innerHTML =
    pane('refund, for now', hl(refund)) +
    pane('~/shop', run('strata holes', D.orders.holes) + '\n' + cmdLine(fill.cmd) + piped(fill.input.trim()) + term(fill.out), { cls: 'term' }) +
    pane('~/shop', cmdLine(bad.cmd) + piped(bad.input.trim()) + term(bad.out), { cls: 'term' });

  $('#ch-views').innerHTML = pane('$ strata view slice:Order --level 0', term(D.orders.levels[0].trimEnd()), { cls: 'term' });

  const R = D.rename, st = structural(R.changes[R.step]), n = diffCount(R.diff);
  $('#ch-diff').innerHTML = `<div class="vs">
    <div class="side old"><h4>✗ line diff <span class="big-n">${n} lines</span></h4>${pane('git diff', diffHtml(R.diff), { count: `<span class="count bad">${n} lines changed</span>` })}</div>
    <div class="side new"><h4>✓ structural change <span class="big-n">${st.count} change</span></h4>${pane('strata diff', st.html, { cls: 'term', count: `<span class="count ok">${st.count} change${st.count > 1 ? 's' : ''}</span>` })}</div>
  </div>`;
}
