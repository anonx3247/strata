// The presentation. Everything it shows comes from site/data/*.js, recorded by
// site/build.mjs from the real CLI; the viewer frames run src/ui/index.html.
// Highlighting and terminal colouring live in common.js.
'use strict';
const D = window.STRATA;
const lastStep = snap => snap.changes.all.steps.at(-1).id;
const stepTitled = (snap, t) => snap.changes.all.steps.find(s => s.title === t).id;
const S = t => D.session.find(s => s.title === t);
const md = s => esc(s).replace(/`([^`]+)`/g, '<code>$1</code>');

// ───────────────────────── a structural change, from the viewer's /api/changes ─────────────────────────
// The headline says what happened in one line (a rename is "Order.Shipped → Order.Sent");
// the code is there on demand.
function segs(ss) {
  return ss.map(([k, t]) => k === '=' ? esc(t) : k === '+' ? `<span class="ins">${esc(t)}</span>` : `<span class="del">${esc(t)}</span>`).join('');
}
function codeHtml(it) {
  return it.lines.map(l => {
    if (l.s === 'fold') return `<span class="fold">  … ${esc(l.t)}</span>`;
    if (l.s === '-') return `<span class="del">${esc(l.t)}</span>`;
    if (l.s === '~' && l.segs) return `<span class="mark">${segs(l.segs)}</span>`;
    if (l.s === '+') return `<span class="ins">${esc(l.t)}</span>${l.note ? `  <span class="note">◀ ${esc(l.note)}</span>` : ''}`;
    return esc(l.t);
  }).join('\n');
}
const KINDS = { fn: 'fn', type: 'type', test: 'test', trait: 'trait', impl: 'impl' };
/** One line saying what happened to a definition, read from the change's own words. */
function headline(it, c) {
  const kind = `<span class="pill">${esc(KINDS[it.kind] || it.kind)}</span>`;
  for (const w of it.what) {
    const m = w.match(/^(variant|field|method) `([^`]+)` renamed to `([^`]+)`/);
    if (m) return { cls: 'rename', html: `<div class="hl-tag">rename · ${m[1]}</div><div class="hl-big">${esc(it.label)}.<span class="from">${esc(m[2])}</span> <span class="arrow">→</span> ${esc(it.label)}.<span class="to">${esc(m[3])}</span></div>` };
  }
  const noted = it.lines.find(l => l.note);
  if (noted) {
    const cond = noted.t.trim().replace(/^(requires|ensures)\s+/, '').replace(/\s*\{$/, '');
    const isReq = /^\s*requires/.test(noted.t);
    const users = it.affected?.map(a => a.name) || [];
    const why = isReq ? `Every caller must now guarantee <code>${esc(cond)}</code>.` : `Callers can now rely on <code>${esc(cond)}</code>.`;
    return { cls: 'contract', html: `<div class="hl-tag warn">⚠ ${esc(noted.note)}</div><div class="hl-big">${kind} ${esc(it.label)} <span class="hl-clause"><b>${esc(noted.t.trim().replace(/\s*\{$/, ''))}</b></span></div>
      <div class="hl-why">${why}${users.length ? ` ${users.length === 1 ? 'Its user is' : `Its ${users.length} users are`} re-checked: ${users.map(u => `<code>${esc(u)}</code>`).join(' ')}.` : ''}</div>` };
  }
  if (it.what.includes('signature changed')) {
    const sig = it.lines.find(l => l.s === '~' && l.segs);
    return { cls: 'sig', html: `<div class="hl-tag">signature changed</div><div class="hl-big code">${sig ? segs(sig.segs).replace(/\s*\{\s*$/, '') : esc(it.label)}</div>${it.what.includes('body changed') ? '<div class="hl-why">and its body changed</div>' : ''}` };
  }
  if (it.mk === 'add') return { cls: 'add', html: `<div class="hl-tag ok">added</div><div class="hl-big">${kind} ${esc(it.label)}</div>` };
  return { cls: 'body', html: `<div class="hl-tag">${it.what.map(esc).join(' · ')}</div><div class="hl-big">${kind} ${esc(it.label)}</div>${it.detail.length ? `<div class="hl-why">${it.detail.map(esc).join(' · ')}</div>` : ''}` };
}
function structural(c) {
  const items = c.sections.flatMap(s => s.items);
  const ifc = items.filter(i => i.interface_changed);
  // the names an interface change is about: the definition's own, and any new name it gave
  const names = ifc.flatMap(m => [m.label, ...m.what.flatMap(w => [...w.matchAll(/renamed to `([^`]+)`/g)].map(x => x[1]))]);
  // a definition that only changed to keep up with another's new interface follows it
  const follows = i => !i.interface_changed && i.lines.some(l => '~+'.includes(l.s) && names.some(n => new RegExp(`\\b${n}\\b`).test(l.t)));
  const main = items.filter(i => !follows(i)), followers = items.filter(follows);
  const hs = main.map(it => headline(it, c));
  const followLine = [
    c.summary.follow_renames ? `${c.summary.follow_renames + followers.length} definitions only follow the rename` : '',
    followers.length && !c.summary.follow_renames ? `call sites updated to follow it: ${followers.map(f => `<code>${esc(f.label)}</code>`).join(' ')}` : '',
  ].filter(Boolean).map(t => `<div class="hl-follow">↳ ${t}</div>`).join('');
  const ver = c.verification?.length ? `<div class="hl-ver">${c.verification.map(v => `<span><code>${esc(v.name)}</code> ${esc(v.before)} → <b class="${/counter|fail/.test(v.after) ? 'o-bad' : /proven/.test(v.after) ? 'o-ok' : ''}">${esc(v.after)}</b></span>`).join('')}</div>` : '';
  const code = items.map(it => `<div class="change"><div class="head"><span>${esc(it.label)}</span><span class="pill">${esc(it.kind)}</span><span class="what">${it.what.map(md).join(' · ')}</span></div><pre>${codeHtml(it)}</pre></div>`).join('');
  return {
    count: main.length,
    html: hs.map(h => `<div class="headline ${h.cls}">${h.html}</div>`).join('') + followLine + ver +
      `<details class="code-details"><summary>show the code</summary>${code}</details>`,
  };
}

// ───────────────────────── steppers: a bar of steps, arrows, and the ← → keys ─────────────────────────
const STEPPERS = [];
class Stepper {
  /** `bar` gets the step segments, `controls` the arrows; `go(i)` shows step i. */
  constructor(root, titles, go, { play = false } = {}) {
    this.root = root; this.n = titles.length; this.cur = -1; this.go_ = go; this.playing = false;
    $('.steps', root).innerHTML = titles.map((t, i) => `<button data-i="${i}"><span class="tip">${i + 1}. ${esc(t)}</span></button>`).join('');
    $('.controls', root).innerHTML = `<button class="btn ghost" data-a="prev" title="previous (←)">←</button>${play ? '<button class="btn" data-a="play">▶ play</button>' : ''}<button class="btn ${play ? 'ghost' : ''}" data-a="next" title="next (→)">→</button><span class="n"></span><span class="keys">← → keys</span>`;
    $('.steps', root).addEventListener('click', e => { const b = e.target.closest('button'); if (b) { this.stop(); this.go(+b.dataset.i); } });
    $('.controls', root).addEventListener('click', e => {
      const a = e.target.closest('[data-a]')?.dataset.a;
      if (a === 'prev') this.prev(); else if (a === 'next') this.next(); else if (a === 'play') this.playing ? this.stop() : this.play();
    });
    STEPPERS.push(this);
  }
  go(i) {
    if (i < 0 || i >= this.n) return;
    this.cur = i;
    $$('.steps button', this.root).forEach((b, j) => { b.classList.toggle('on', j === i); b.classList.toggle('done', j < i); b.style.setProperty('--p', this.playing ? 0 : 1); });
    $('.controls .n', this.root).textContent = `${i + 1} / ${this.n}`;
    $('[data-a=prev]', this.root).disabled = i === 0;
    $('[data-a=next]', this.root).disabled = i === this.n - 1;
    this.go_(i);
  }
  prev() { this.stop(); this.go(this.cur - 1); }
  next() { this.stop(); this.go(this.cur + 1); }
  play() { this.playing = true; const b = $('[data-a=play]', this.root); if (b) b.textContent = '❚❚ pause'; this.go(this.cur + 1 < this.n ? this.cur + 1 : 0); }
  stop() { this.playing = false; const b = $('[data-a=play]', this.root); if (b) b.textContent = '▶ play'; $$('.steps button', this.root).forEach(b => b.style.setProperty('--p', 1)); }
  /** While playing: fill the current segment over `ms`, then go on. Resolves false if interrupted. */
  async wait(ms, i) {
    const btn = $$('.steps button', this.root)[i], t0 = performance.now();
    while (performance.now() - t0 < ms) { if (!this.playing || this.cur !== i) return; btn.style.setProperty('--p', (performance.now() - t0) / ms); await sleep(50); }
    if (this.playing && this.cur === i) { if (i + 1 < this.n) this.go(i + 1); else this.stop(); }
  }
}
// ← → drive whichever stepper takes up most of the screen
addEventListener('keydown', e => {
  if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
  if (e.metaKey || e.ctrlKey || e.altKey || /INPUT|TEXTAREA|SELECT/.test(e.target.tagName)) return;
  const seen = s => { const r = s.root.getBoundingClientRect(); return Math.max(0, Math.min(r.bottom, innerHeight) - Math.max(r.top, 0)); };
  const best = STEPPERS.map(s => [s, seen(s)]).sort((a, b) => b[1] - a[1])[0];
  if (!best || best[1] < innerHeight * 0.25) return;
  e.preventDefault();
  e.key === 'ArrowLeft' ? best[0].prev() : best[0].next();
});
/** Type a command into a terminal, then what was piped in, then its output; stops if `alive()` turns false. */
async function typeInto(body, s, alive = () => true) {
  body.innerHTML = '';
  const line = document.createElement('span'); line.className = 'term-line'; line.innerHTML = '<span class="prompt">$</span> <span class="cmd"></span>';
  body.appendChild(line);
  for (const ch of s.cmd) { if (!alive()) return false; $('.cmd', line).textContent += ch; await sleep(20); }
  if (s.input) { body.insertAdjacentHTML('beforeend', piped(s.input)); await sleep(250); }
  for (const l of s.out.split('\n')) {
    if (!alive()) return false;
    body.insertAdjacentHTML('beforeend', `<span class="term-line">${termLine(l)}</span>`);
    body.scrollTop = body.scrollHeight; await sleep(45);
  }
  return true;
}

// ───────────────────────── scroll reveal, nav, progress ─────────────────────────
for (const el of $$('.words')) el.innerHTML = el.textContent.split(/(\s+)/).map((w, i) => /\s/.test(w) ? w : `<span class="w" style="transition-delay:${i * 28}ms">${esc(w)}</span>`).join('');
for (const el of $$('.stagger')) [...el.children].forEach((c, i) => (c.style.transitionDelay = `${i * 120}ms`));
const io = new IntersectionObserver(es => es.forEach(e => { if (e.isIntersecting) { e.target.classList.add('in'); e.target.dispatchEvent(new Event('enter')); io.unobserve(e.target); } }), { threshold: 0.15 });
$$('.reveal, .words, .stagger, .lanes').forEach(el => io.observe(el));
const secs = $$('section[data-nav]');
$('#nav').innerHTML = secs.map(s => `<a href="#${s.id}"><span>${esc(s.dataset.nav)}</span></a>`).join('');
const navIo = new IntersectionObserver(es => es.forEach(e => { if (e.isIntersecting) $$('#nav a').forEach(a => a.classList.toggle('on', a.hash === '#' + e.target.id)); }), { rootMargin: '-45% 0px -50% 0px' });
secs.forEach(s => navIo.observe(s));
addEventListener('scroll', () => { const h = document.documentElement; $('#progress').style.width = (h.scrollTop / (h.scrollHeight - h.clientHeight)) * 100 + '%'; }, { passive: true });

// ───────────────────────── hero ─────────────────────────
$('#title').innerHTML = [...'Strata'].map((c, i) => `<span class="l" style="animation-delay:${0.1 + i * 0.08}s">${c}</span>`).join('');
(async () => {
  const el = $('#tagline');
  const parts = [['A language for working with agents. ', ''], ['Code that says what it means, checked and proven as it is written.', 'dim']];
  await sleep(800);
  for (const [t, cls] of parts) {
    const span = document.createElement('span'); if (cls) span.className = cls; el.appendChild(span);
    for (const ch of t) { span.textContent += ch; await sleep(ch === ' ' ? 18 : 26); }
  }
  el.classList.add('done');
  $('#hero .lede').classList.add('in');
})();
// layered bands, drifting
(() => {
  const svg = $('#bands');
  const colors = ['#1a1d2b', '#1d2033', '#22233a', '#2a2540', '#3a2c3c', '#4d3438', '#5e3f33'];
  const n = colors.length, W = 1600, paths = [];
  for (let i = 0; i < n; i++) {
    const p = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    p.setAttribute('fill', colors[i]); p.setAttribute('opacity', 0.55 + i * 0.05);
    svg.appendChild(p); paths.push(p);
  }
  let t = 0, scrollY = 0;
  addEventListener('scroll', () => (scrollY = window.scrollY), { passive: true });
  function frame() {
    t += 0.0025;
    paths.forEach((p, i) => {
      const base = 520 + i * 70 + scrollY * (0.08 + i * 0.03);
      let d = `M0 ${base}`;
      for (let x = 0; x <= W; x += 80) {
        const y = base + Math.sin(x / 260 + t * (1 + i * 0.3) + i) * (18 + i * 4) + Math.sin(x / 90 + t * 2 + i * 2) * 4;
        d += ` L${x} ${y.toFixed(1)}`;
      }
      p.setAttribute('d', d + ` L${W} 1000 L0 1000 Z`);
    });
    if (scrollY < innerHeight * 1.2) requestAnimationFrame(frame); else setTimeout(() => requestAnimationFrame(frame), 300);
  }
  frame();
})();

// ───────────────────────── the real viewer, in a frame ─────────────────────────
class Viewer {
  constructor(el, label) {
    this.el = el; this.w = +el.dataset.w || 1400; this.h = +el.dataset.h || 900;
    this.ready = false; this.pending = null; this.views = el.dataset.views;
    el.innerHTML = `<div class="chrome"><span class="dots"><i></i><i></i><i></i></span><span class="url">${esc(label)}</span><span class="live">live</span></div><div class="stage"><div class="hint">interactive · click around</div></div>`;
    this.stage = $('.stage', el);
    this.iframe = document.createElement('iframe');
    this.iframe.title = 'Strata viewer'; this.iframe.width = this.w; this.iframe.height = this.h;
    this.stage.appendChild(this.iframe);
    addEventListener('message', e => {
      if (e.source !== this.iframe.contentWindow || e.data?.type !== 'strata-ready') return;
      this.ready = true; if (this.pending) this.post(this.pending);
    });
    new ResizeObserver(() => this.fit()).observe(el);
    const lazy = new IntersectionObserver(es => { if (es[0].isIntersecting) { this.iframe.src = 'viewer.html#level=1'; lazy.disconnect(); } }, { rootMargin: '600px' });
    lazy.observe(el);
    this.stage.addEventListener('pointerenter', () => { $('.hint', el).style.opacity = 0; }, { once: true });
  }
  fit() {
    const s = Math.min(1, this.el.clientWidth / this.w);
    this.iframe.style.transform = `scale(${s})`;
    this.stage.style.height = this.h * s + 'px';
  }
  setLabel(t) { $('.url', this.el).textContent = t; }
  /** Show a recorded snapshot: {state, changes}, and what to select. */
  show(snap, opts = {}) {
    this.pending = { type: 'strata-snap', state: snap.state, changes: snap.changes, ...opts };
    if (this.ready) this.post(this.pending);
  }
  post(m) { this.iframe.contentWindow.postMessage(m, '*'); }
}

// ───────────────────────── fix 1: structural diffs ─────────────────────────
(() => {
  const rename = S('A rename is one change'), fill = S('It fills them, and closes the change'), fix = S('It fixes it, and proves it');
  const C = D.contract, Cs = t => C.steps.find(s => s.title === t);
  const cases = [
    { tab: 'Rename a variant', linediff: rename.linediff, snap: rename, step: lastStep(rename),
      note: `<b>strata rename Shipped Sent</b>. As text, five definitions change, and a line in a test re-wraps because <code>Sent</code> is shorter. What happened is one rename.` },
    { tab: 'Add a parameter', linediff: C.paramDiff, snap: C, step: stepTitled(C, 'discount: members'),
      note: `<code>discount</code> takes a new <code>member</code> flag, not used yet. The change is the new parameter; its two call sites only follow it.` },
    { tab: 'Change a signature and its body', linediff: D.orders.couponsDiff, snap: fill, step: stepTitled(fill, 'coupons'),
      note: `<code>total</code> takes a coupon and uses it: the signature and the body both changed, so both are shown. The callers that had to follow are listed, not repeated.` },
    { tab: 'Change only a body', linediff: fix.linediff, snap: fix, step: lastStep(fix),
      note: `The fix to <code>refund</code> changes its body, not its signature or contract: <b>nothing else needs re-checking</b>, and the verdict before and after is part of the change.` },
    { tab: 'Tighten a contract', linediff: Cs('Tighten discount’s contract').linediff, snap: C, step: stepTitled(C, 'discount only from 1.00'),
      note: `A new <code>requires</code> is a <b>stricter precondition</b>: every caller now has more to guarantee, so each of them is re-checked. <a href="#contracts">What happens next ↓</a>` },
  ];
  $('#rename-count').textContent = diffCount(rename.linediff);
  const viewer = new Viewer($('#diff-viewer'), 'strata ui · Changes');
  let cur = -1;
  function pick(i) {
    if (i === cur) return; cur = i;
    const c = cases[i];
    $$('#diff-tabs button').forEach((b, j) => b.classList.toggle('on', j === i));
    $('#ld-body').innerHTML = diffHtml(c.linediff);
    { const n = diffCount(c.linediff); $('#ld-count').textContent = `${n} line${n === 1 ? '' : 's'} changed`; }
    const st = structural(c.snap.changes[c.step]);
    $('#sd-body').innerHTML = st.html;
    $('#sd-count').textContent = `${st.count} change${st.count > 1 ? 's' : ''}`;
    $('#sd-title').textContent = `strata diff · ${c.snap.changes.all.steps.find(s => s.id === c.step).title}`;
    $('#diff-note').innerHTML = c.note;
    viewer.show(c.snap, { level: 'changes', step: c.step, sel: null });
  }
  $('#diff-tabs').innerHTML = cases.map((c, i) => `<button data-i="${i}">${esc(c.tab)}</button>`).join('');
  $('#diff-tabs').addEventListener('click', e => { const b = e.target.closest('button'); if (b) pick(+b.dataset.i); });
  pick(0);
})();

// ───────────────────────── fix 2: the refund explorer ─────────────────────────
(() => {
  const fillStep = S('It fills the hole'), testStep = S('The tests pass'), verStep = S('Verification finds the bug'), fixStep = S('It fixes it, and proves it');
  let fixed = false, verified = false;
  const money = c => (c < 0 ? '−' : '') + (Math.abs(c) / 100).toFixed(2);
  const refund = v => fixed ? Math.max(v - 500, 0) : v - 500;
  const axis = $('#axis'), range = $('#ex-range');
  const pos = v => (v / 2000) * 100 + '%';
  $('#pin-test').style.left = pos(900); $('#pin-cex').style.left = pos(63);
  axis.style.setProperty('--w', pos(500));
  function upd() {
    const v = +range.value, r = refund(v);
    $('#knob').style.left = pos(v);
    $('#knob').style.background = r < 0 ? 'var(--bad)' : 'var(--text)';
    $('#ex-v').textContent = money(v);
    $('#ex-r').textContent = money(r); $('#ex-r').style.color = r < 0 ? 'var(--bad)' : 'var(--ok)';
  }
  range.addEventListener('input', upd); upd();
  const code = () => (fixed ? fixStep : fillStep).input;
  $('#ex-code').innerHTML = hl(code());
  async function type(cmd, out) {
    const el = $('#ex-term');
    el.innerHTML = cmdLine(cmd);
    await sleep(350);
    for (const l of out.split('\n')) { el.insertAdjacentHTML('beforeend', `<span class="term-line">${termLine(l)}</span>`); await sleep(70); }
  }
  $('#ex-test').onclick = () => type(testStep.cmd, testStep.out);
  $('#ex-verify').onclick = async () => {
    if (fixed) { await type('strata verify refund', fixStep.out.split('$ strata verify refund\n')[1]); return; }
    axis.classList.remove('scanning'); void axis.offsetWidth; axis.classList.add('scanning');
    await type(verStep.cmd, '');
    await sleep(1300);
    axis.classList.add('scanned'); verified = true;
    range.value = 63; upd();
    for (const l of verStep.out.split('\n')) { $('#ex-term').insertAdjacentHTML('beforeend', `<span class="term-line">${termLine(l)}</span>`); await sleep(90); }
    $('#ex-fix').disabled = false;
  };
  $('#ex-fix').onclick = async () => {
    fixed = true; $('#ex-fix').disabled = true;
    $('#ex-code').innerHTML = hl(code());
    axis.classList.remove('scanned'); $('.danger', axis).style.width = '0';
    $('#pin-cex').style.opacity = 0;
    upd();
    await type(fixStep.cmd, fixStep.out);
  };
  // the tour of math.strata
  const mv = $('#math-verify');
  const lines = D.math.verify.split('\n');
  mv.innerHTML = lines.map(l => `<span class="term-line hide">${termLine(l)}</span>`).join('');
  mv.closest('.reveal').addEventListener('enter', async () => { for (const el of $$('.term-line', mv)) { el.classList.remove('hide'); await sleep(110); } });
  new Viewer($('#math-viewer'), 'strata ui · examples/math.strata').show(D.math, { sel: 'next_digit' });
})();

// ───────────────────────── fix 3: contracts ─────────────────────────
(() => {
  const C = D.contract, g = $('#cgraph');
  const nodes = {
    discount: { x: 20, y: 50, t: 'discount', s: 'the contract changes here' },
    checkout: { x: 72, y: 22, t: 'checkout', s: 'calls discount(…)' },
    totals: { x: 72, y: 78, t: 'test "totals"', s: 'calls discount(20000)' },
  };
  const LABEL = { pass: '✓ proven', fail: '✗ counterexample', changed: 'changed', recheck: 're-check', hole: '?hole' };
  g.innerHTML = `<svg><path id="e1"/><path id="e2"/></svg>` + Object.entries(nodes).map(([k, n]) => `<div class="node" id="n-${k}" style="left:${n.x}%;top:${n.y}%"><span class="badge"></span>${esc(n.t)}<small>${esc(n.s)}</small></div>`).join('');
  function edges() {
    const W = g.clientWidth, H = g.clientHeight, p = n => [nodes[n].x / 100 * W, nodes[n].y / 100 * H];
    const e = (a, b) => { const [x1, y1] = p(a), [x2, y2] = p(b); return `M${x1 + 70} ${y1} C${(x1 + x2) / 2} ${y1}, ${(x1 + x2) / 2} ${y2}, ${x2 - 80} ${y2}`; };
    $('#e1').setAttribute('d', e('discount', 'checkout')); $('#e2').setAttribute('d', e('discount', 'totals'));
  }
  new ResizeObserver(edges).observe(g);
  const body = $('#c-term');
  let token = 0;
  const st = new Stepper($('#c-player'), C.steps.map(s => s.title), async i => {
    const s = C.steps[i], my = ++token;
    for (const k of Object.keys(nodes)) {
      const v = s.g[k] || '';
      $('#n-' + k).className = 'node ' + v;
      $('#n-' + k + ' .badge').textContent = LABEL[v] || '';
    }
    $$('#cgraph path').forEach(p => { p.classList.toggle('lit', !!s.g.lit); p.style.stroke = ''; });
    if (s.g.bad) $('#e1').style.stroke = 'var(--bad)';
    $('#c-say').innerHTML = `<h3>${esc(s.title)}</h3><p>${md(s.say)}</p>`;
    await typeInto(body, s, () => my === token);
  });
  const o = new IntersectionObserver(es => { if (es[0].isIntersecting) { o.disconnect(); st.go(0); } }, { threshold: 0.3 });
  o.observe($('#c-player'));
})();

// ───────────────────────── the agent session ─────────────────────────
(() => {
  const ss = D.session, viewer = new Viewer($('#sess-viewer'), 'strata ui · ~/shop');
  let token = 0;
  const st = new Stepper($('#session'), ss.map(s => s.title), async i => {
    const s = ss[i], my = ++token;
    const who = s.who === 'you' ? 'you' : s.who === 'agent B' ? 'agentb' : '';
    $('#sess-say').innerHTML = `<div class="who ${who}"><i></i>${esc(s.who)}</div><h3>${esc(s.title)}</h3><p>${md(s.say)}</p>`;
    $('#sess-diffbox').style.display = s.linediff ? '' : 'none';
    if (s.linediff) $('#sess-diff').innerHTML = diffHtml(s.linediff);
    viewer.show(s, { sel: s.sel, level: 1 });
    const done = await typeInto($('#sess-term .body'), s, () => my === token);
    if (done && s.code) $('#sess-term .body').insertAdjacentHTML('beforeend', `<span class="term-line o-dim">exit ${s.code}</span>`);
    if (done && st.playing) st.wait(5200 + s.say.length * 18, i);
  }, { play: true });
  // start playing when it is first seen
  const o = new IntersectionObserver(es => { if (es[0].isIntersecting) { o.disconnect(); st.play(); } }, { threshold: 0.35 });
  o.observe($('#session .player'));
})();

// ───────────────────────── merge ─────────────────────────
(() => {
  const M = D.orders.merge;
  $('#m-a').innerHTML = cmdLine(M.a.cmd) + term(M.a.out);
  $('#m-b').innerHTML = cmdLine(M.b.cmd) + piped(M.b.input) + term(M.b.out);
  $('#m-sync').innerHTML = cmdLine('strata sync main') + term(M.sync) + `<span class="term-line o-dim">\n// the merged result</span>` + hl(M.result);
  $('#lanes').innerHTML = `<svg viewBox="0 0 1200 190" preserveAspectRatio="xMidYMid meet">
    <text class="lane-label" x="0" y="54">main</text><text class="lane-label" x="0" y="144">agent-b</text>
    <path d="M90 50 L1130 50" stroke="#8ea2ff" stroke-width="2.5" fill="none" class="draw"/>
    <path d="M250 50 C 330 50, 330 140, 410 140 L 820 140 C 900 140, 900 50, 980 50" stroke="#63e6be" stroke-width="2.5" fill="none" class="draw"/>
    <g class="commit anim" style="transition-delay:.2s"><circle cx="250" cy="50" r="9" stroke="#8ea2ff"/><text x="232" y="28">shared</text></g>
    <g class="commit anim" style="transition-delay:.7s"><circle cx="600" cy="50" r="9" stroke="#8ea2ff"/><text x="520" y="28">A: rename line_total → line_cost</text></g>
    <g class="commit anim" style="transition-delay:1s"><circle cx="600" cy="140" r="9" stroke="#63e6be"/><text x="520" y="174">B: rewrite line_total's body</text></g>
    <g class="commit anim" style="transition-delay:1.6s"><circle cx="980" cy="50" r="12" stroke="#5fd07a" style="fill:#5fd07a33"/><text x="935" y="28" style="fill:#5fd07a">✓ merged, checked</text></g>
  </svg>`;
})();

// ───────────────────────── levels ─────────────────────────
(() => {
  const L = D.orders.levels;
  const info = [['Level 0', 'a map: one line per definition, its dependencies and status'], ['Level 1', 'interfaces: types, signatures, contracts'], ['Level 2', 'an outline: a one-line summary of each body'], ['Level 3', 'the full source, which parses back to the same tree']];
  $('#lv-pick').innerHTML = info.map(([t, d], i) => `<button data-l="${i}"><b>${t}</b><span>${d}</span></button>`).join('');
  function pick(l) {
    $$('#lv-pick button').forEach((b, i) => b.classList.toggle('on', i === l));
    const body = $('#lv-body'); body.style.opacity = 0;
    setTimeout(() => { body.innerHTML = hl(L[l].trimEnd()); body.style.opacity = 1; }, 160);
    $('#lv-title').textContent = `$ strata view slice:Order --level ${l}`;
    $('#lv-count').textContent = `${L[l].trimEnd().split('\n').length} lines`;
  }
  $('#lv-pick').addEventListener('click', e => { const b = e.target.closest('button'); if (b) pick(+b.dataset.l); });
  pick(1);
})();

// ───────────────────────── explore: small examples ─────────────────────────
(() => {
  const ex = [
    { id: 'bank', tab: 'bank: money that can’t go negative', sel: 'transfer',
      say: '`Cents` is an `Int` that is never negative. `withdraw` and `transfer` say in their contracts what happens to the balances, and the verifier proves it: a transfer never creates or loses money.' },
    { id: 'traits', tab: 'traits: laws that are checked', sel: 'impl Combine for Midpoint',
      say: 'A trait can state laws. `Combine` says `combine` is associative; the verifier checks every implementation, and finds that averaging two numbers isn’t.' },
    { id: 'wc', tab: 'wc: capabilities as parameters', sel: 'main',
      say: 'The Unix word count. `main` asks for exactly what it touches (its options, the console, the files), so its signature says what it can do, and the tests hand it stand-ins.' },
  ];
  const viewer = new Viewer($('#explore-viewer'), 'strata ui');
  function pick(i) {
    const e = ex[i];
    $$('#ex-tabs button').forEach((b, j) => b.classList.toggle('on', j === i));
    $('#ex-say').innerHTML = md(e.say);
    viewer.setLabel(`strata ui · examples/${e.id}`);
    viewer.show(D.explore[e.id], { sel: e.sel, level: 1 });
  }
  $('#ex-tabs').innerHTML = ex.map((e, i) => `<button data-i="${i}">${esc(e.tab)}</button>`).join('');
  $('#ex-tabs').addEventListener('click', e => { const b = e.target.closest('button'); if (b) pick(+b.dataset.i); });
  pick(0);
})();

$('#start-cmds').innerHTML = [
  ['cargo build --release', ''], ['alias strata=$PWD/target/release/strata', ''],
  ['strata --file examples/math.strata verify', 'prove what can be proven, fuzz the rest'],
  ['strata init && strata add examples/orders.strata', 'a package: every change checked and logged'],
  ['strata holes', 'what is left to implement'],
  ['strata ui', 'the live viewer, on http://127.0.0.1:7777'],
].map(([c, n]) => cmdLine(c).replace('</span></span>', `</span>${n ? `  <span class="c">// ${esc(n)}</span>` : ''}</span>`)).join('');
