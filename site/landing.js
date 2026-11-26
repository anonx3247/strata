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

// ───────── the agent loop: steps of the recorded session (data/session.js) ─────────
{
  const S = STRATA.session, md = t => esc(t).replace(/`([^`]+)`/g, '<code>$1</code>');
  const at = t => S.find(s => s.title === t);
  const steps = [
    ['The agent asks what is left', at('The agent asks what is left to do')],
    ['It fills the hole; the edit is checked', at('It fills the hole')],
    ['Verification finds the bug the test missed', at('Verification finds the bug')],
    ['It fixes it, and it is proven', at('It fixes it, and proves it')],
    ['An edit that doesn\'t check never gets in', at('A broken edit is refused')],
  ];
  $('#loop').innerHTML = steps.map(([h, s], i) => `
    <div class="step${s.code ? ' bad' : ''}">
      <div class="n">${i + 1}</div><h3>${esc(h)}</h3><p>${md(s.say)}</p>
      <div class="pane"><div class="bar"><span class="dots"><i></i><i></i><i></i></span><span class="title">~/shop</span></div>
        <div class="body wrap">${cmdLine(s.cmd)}${s.input ? piped(s.input.trim()) : ''}${term(s.out)}</div></div>
    </div>`).join('');
}

// ───────── a rename: lines of text, or one change (data/rename.js) ─────────
{
  const R = STRATA.rename, n = diffCount(R.diff);
  $('#rename').innerHTML = `
    <div class="side old"><div class="big"><b>${n}</b> changed lines</div><p>A text diff of renaming a type: every file and line that mentions it.</p>
      <div class="pane"><div class="bar"><span class="title">git diff</span><span class="count bad">${n} lines</span></div><div class="body">${diffHtml(R.diff)}</div></div></div>
    <div class="side new"><div class="big"><b>1</b> change</div><p>The same change in Strata: a rename, with the definitions it reached.</p>
      <div class="pane"><div class="bar"><span class="title">~/tasks</span><span class="count ok">1 change</span></div><div class="body wrap">${cmdLine(R.cmd)}${term(R.out)}</div></div></div>`;
}
