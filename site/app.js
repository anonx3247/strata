// The presentation. Everything it shows comes from site/data/*.js, recorded by
// site/build.mjs from the real CLI; the viewer frames run src/ui/index.html.
'use strict';
const D = window.STRATA;
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const sleep = ms => new Promise(r => setTimeout(r, ms));
const lastStep = snap => snap.changes.all.steps.at(-1).id;
const stepTitled = (snap, t) => snap.changes.all.steps.find(s => s.title === t).id;
const S = t => D.session.find(s => s.title === t);

// ───────────────────────── highlighting ─────────────────────────
const KW = new Set('fn type trait impl for in if else match while loop return break continue mut let pub use test assert requires ensures where it self true false mock distinct derive or except invariant law'.split(' '));
function hl(code) {
  const re = /(\/\/[^\n]*)|("(?:[^"\\]|\\.)*")|(\?[a-z_][a-z0-9_]*)|(\b\d+\b)|([A-Za-z_][A-Za-z0-9_]*)|([\s\S])/g;
  let out = '', m;
  while ((m = re.exec(code))) {
    if (m[1]) out += `<span class="c">${esc(m[1])}</span>`;
    else if (m[2]) out += `<span class="s">${esc(m[2])}</span>`;
    else if (m[3]) out += `<span class="h">${esc(m[3])}</span>`;
    else if (m[4]) out += `<span class="nm">${m[4]}</span>`;
    else if (m[5]) out += KW.has(m[5]) ? `<span class="k">${m[5]}</span>` : /^[A-Z]/.test(m[5]) ? `<span class="t">${m[5]}</span>` : esc(m[5]);
    else out += esc(m[6]);
  }
  return out;
}
/** A line of terminal output, coloured as the meaning of strata's output suggests. */
function termLine(l) {
  const e = esc(l);
  if (/^✓/.test(l)) return `<span class="o-ok">${e}</span>`;
  if (/^✗|^error|^rejected/.test(l)) return `<span class="o-bad">${e}</span>`;
  if (/^[~○]/.test(l)) return `<span class="o-warn">${e}</span>`;
  if (/^\s+input:/.test(l)) return `<span class="o-in">${e}</span>`;
  if (/^committed to git|^Auto-merging|^Automatic merge/.test(l)) return `<span class="o-dim">${e}</span>`;
  if (/^\s*\?[a-z_]/.test(l)) return e.replace(/(\?[a-z_0-9]+)/, '<span class="h">$1</span>');
  if (/^strata: .*applied/.test(l)) return `<span class="o-acc">${e}</span>`;
  if (/^(fn|type|test|trait|impl)\s+\S/.test(l)) return e.replace(/(#[0-9a-f]{8,})/g, '<span class="o-dim">$1</span>').replace(/(added|changed)/, '<b>$1</b>');
  if (/^\$ /.test(l)) return `<span class="prompt">$</span> <span class="cmd">${esc(l.slice(2))}</span>`;
  return e.replace(/(#[0-9a-f]{8,})/g, '<span class="o-dim">$1</span>');
}
const term = out => out.split('\n').map(l => `<span class="term-line">${termLine(l)}</span>`).join('');
const cmdLine = c => `<span class="term-line"><span class="prompt">$</span> <span class="cmd">${esc(c)}</span></span>`;
const piped = src => `<span class="piped">${hl(src)}</span>`;
function diffHtml(d) {
  return d.split('\n').map(l => {
    const c = /^(\+\+\+|---)/.test(l) ? 'd-file' : l[0] === '+' ? 'd-add' : l[0] === '-' ? 'd-del' : l[0] === '@' ? 'd-hunk' : 'd-ctx';
    return `<span class="${c}">${esc(l) || ' '}</span>`;
  }).join('');
}
const diffCount = d => d.split('\n').filter(l => /^[+-]/.test(l) && !/^(\+\+\+|---)/.test(l)).length;

// ───────────────────────── a structural change, from the viewer's /api/changes ─────────────────────────
function segs(ss) {
  return ss.map(([k, t]) => k === '=' ? esc(t) : k === '+' ? `<span class="ins">${esc(t)}</span>` : '').join('');
}
function changeHtml(it) {
  const pill = it.mk === 'add' ? 'ok' : it.mk === 'del' ? 'bad' : it.interface_changed ? 'warn' : '';
  const lines = it.lines.map(l => {
    if (l.s === 'fold') return `<span class="fold">  … ${esc(l.t)}</span>`;
    if (l.s === '-') return null;
    if (l.s === '~' && l.segs) return `<span class="mark">${segs(l.segs)}</span>`;
    if (l.s === '+') return `<span class="ins">${esc(l.t)}</span>`;
    return esc(l.t);
  }).filter(x => x != null).join('\n');
  const updated = it.affected?.filter(a => a.updated).map(a => a.name) || [];
  return `<div class="change"><div class="head"><span>${esc(it.label)}</span><span class="pill ${pill}">${esc(it.kind)}</span></div>
    <div class="what">${it.what.map(w => esc(w).replace(/`([^`]+)`/g, '<code>$1</code>')).join(' · ')}</div>
    ${it.detail.length ? `<div class="detail">${it.detail.map(esc).join(' · ')}</div>` : ''}
    ${updated.length ? `<div class="detail">updated with it: ${updated.map(n => `<code>${esc(n)}</code>`).join(' ')}</div>` : ''}
    <pre>${lines}</pre></div>`;
}
function structural(c) {
  const items = c.sections.flatMap(s => s.items);
  const sum = c.summary;
  const parts = [sum.added && `${sum.added} added`, sum.changed && `${sum.changed} changed`, sum.renamed && `${sum.renamed} renamed`, sum.interface_changes && `${sum.interface_changes} interface change${sum.interface_changes > 1 ? 's' : ''}`].filter(Boolean);
  const follow = sum.follow_renames ? `<div class="detail" style="margin:4px 0 14px">+ ${sum.follow_renames} definitions that only follow the rename, folded into it</div>` : '';
  const ver = c.verification?.length ? `<div class="change"><div class="head">verification</div><pre>${c.verification.map(v => `${esc(v.name.padEnd(12))} ${esc(v.before)} → <b>${esc(v.after)}</b>`).join('\n')}</pre></div>` : '';
  return { html: `<div class="detail" style="margin-bottom:12px;font-family:var(--mono);font-size:12.5px">${parts.join(' · ')}</div>` + follow + items.map(changeHtml).join('') + ver, count: items.length };
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
  const C = D.contract;
  const cases = [
    { tab: 'Rename a variant', linediff: rename.linediff, snap: rename, step: lastStep(rename),
      note: `<b>strata rename Shipped Sent</b>: the line diff touches five definitions and re-wraps a line in a test, because <code>Sent</code> is shorter. Strata reports one rename; the definitions that only follow it are folded in.` },
    { tab: 'Add a parameter', linediff: D.orders.couponsDiff, snap: fill, step: stepTitled(fill, 'coupons'),
      note: `<code>total</code> takes a coupon now. Strata separates the <b>interface change</b> (what other code relies on) from the bodies that had to follow it, and says it was one change across three definitions.` },
    { tab: 'Change only a body', linediff: fix.linediff, snap: fix, step: lastStep(fix),
      note: `The fix to <code>refund</code> changes its body, not its signature or contract: <b>nothing else needs re-checking</b>, and the verdict before and after is part of the change.` },
    { tab: 'Tighten a contract', linediff: C.linediff, snap: C, step: lastStep(C),
      note: `A new <code>requires</code> is a <b>stricter precondition</b>: Strata marks it so, and lists who uses the definition. <a href="#contracts">More on contracts ↓</a>` },
  ];
  $('#rename-count').textContent = diffCount(rename.linediff);
  const viewer = new Viewer($('#diff-viewer'), 'strata ui · Changes');
  let cur = -1;
  function pick(i) {
    if (i === cur) return; cur = i;
    const c = cases[i];
    $$('#diff-tabs button').forEach((b, j) => b.classList.toggle('on', j === i));
    $('#ld-body').innerHTML = diffHtml(c.linediff);
    $('#ld-count').textContent = `${diffCount(c.linediff)} lines changed`;
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
  const C = D.contract;
  const g = $('#cgraph');
  const nodes = {
    discount: { x: 22, y: 50, t: 'discount', s: 'fn · the contract changes' },
    checkout: { x: 70, y: 22, t: 'checkout', s: 'discount(total(lines))' },
    totals: { x: 70, y: 78, t: 'test "totals"', s: 'discount(20000)' },
  };
  g.innerHTML = `<svg><path id="e1"/><path id="e2"/></svg>` + Object.entries(nodes).map(([k, n]) => `<div class="node" id="n-${k}" style="left:${n.x}%;top:${n.y}%">${esc(n.t)}<small>${esc(n.s)}</small></div>`).join('');
  function edges() {
    const W = g.clientWidth, H = g.clientHeight, p = n => [nodes[n].x / 100 * W, nodes[n].y / 100 * H];
    const e = (a, b) => { const [x1, y1] = p(a), [x2, y2] = p(b); return `M${x1 + 60} ${y1} C${(x1 + x2) / 2} ${y1}, ${(x1 + x2) / 2} ${y2}, ${x2 - 70} ${y2}`; };
    $('#e1').setAttribute('d', e('discount', 'checkout')); $('#e2').setAttribute('d', e('discount', 'totals'));
  }
  new ResizeObserver(edges).observe(g);
  $('#c-linediff').innerHTML = diffHtml(C.linediff);
  $('#c-diff').innerHTML = structural(C.changes[lastStep(C)]).html;
  const steps = [
    { btn: '1 · checkout, as it is', run: () => { $('#c-term').innerHTML = cmdLine('strata edit -m "checkout"') + piped(C.checkout.input) + term(C.checkout.out) + cmdLine('strata verify checkout') + term(C.checkout.verify); $('#n-checkout').className = 'node pass'; },
      say: 'First, as it is today: <code>checkout</code> is proven.' },
    { btn: '2 · edit discount', run: () => { $('#c-term').innerHTML += cmdLine('strata edit -m "discount only from 1.00"') + piped(C.discount.input) + term(C.discount.out); $('#n-discount').className = 'node changed'; $('#n-checkout').className = 'node recheck'; $('#n-totals').className = 'node recheck'; $$('#cgraph path').forEach(p => p.classList.add('lit')); },
      say: 'The interface changed, so the edit names the dependents to re-check: <code>checkout</code> and the test.' },
    { btn: '3 · strata test', run: () => { $('#c-term').innerHTML += cmdLine('strata test') + term(C.test); $('#n-totals').className = 'node pass'; },
      say: 'The tests still pass: the only test calls <code>discount(20000)</code>. Nothing tested an empty basket.' },
    { btn: '4 · strata verify', run: () => { $('#c-term').innerHTML += cmdLine('strata verify') + term(C.verify); $('#n-checkout').className = 'node fail'; $$('#cgraph path').forEach(p => p.classList.remove('lit')); $('#e1').style.stroke = 'var(--bad)'; },
      say: 'Verification checks <code>checkout</code> against the <b>new</b> contract, and finds the call that breaks it: a basket worth 0.00.' },
  ];
  let i = 0;
  const term_ = $('#c-term');
  function reset() { i = 0; term_.innerHTML = '<span class="o-dim">Press the button to step through.</span>'; Object.keys(nodes).forEach(k => ($('#n-' + k).className = 'node')); $$('#cgraph path').forEach(p => { p.classList.remove('lit'); p.style.stroke = ''; }); $('#c-next').textContent = steps[0].btn; $('#c-next').disabled = false; $('#c-say').innerHTML = 'Step through what happens.'; }
  $('#c-next').onclick = () => {
    if (i >= steps.length) return;
    if (i === 0) term_.innerHTML = '';
    steps[i].run(); $('#c-say').innerHTML = steps[i].say;
    i++; $('#c-next').textContent = i < steps.length ? steps[i].btn : 'done';
    $('#c-next').disabled = i >= steps.length;
    term_.scrollTop = term_.scrollHeight;
  };
  $('#c-reset').onclick = reset; reset();
})();

// ───────────────────────── the agent session ─────────────────────────
(() => {
  const ss = D.session, viewer = new Viewer($('#sess-viewer'), 'strata ui · ~/shop');
  let cur = -1, playing = false, token = 0;
  $('#sess-steps').innerHTML = ss.map((s, i) => `<button data-i="${i}" title="${esc(s.title)}"></button>`).join('');
  $('#sess-steps').addEventListener('click', e => { const b = e.target.closest('button'); if (b) { stop(); go(+b.dataset.i); } });
  async function go(i) {
    if (i < 0 || i >= ss.length) return;
    cur = i; const s = ss[i], my = ++token;
    $$('#sess-steps button').forEach((b, j) => { b.classList.toggle('on', j === i); b.classList.toggle('done', j < i); b.style.setProperty('--p', playing ? 0 : 1); });
    const who = s.who === 'you' ? 'you' : s.who === 'agent B' ? 'agentb' : '';
    $('#sess-say').innerHTML = `<div class="who ${who}"><i></i>${esc(s.who)}</div><h3>${esc(s.title)}</h3><p>${esc(s.say).replace(/`([^`]+)`/g, '<code>$1</code>')}</p>`;
    $('#sess-n').textContent = `${i + 1} / ${ss.length}`;
    $('#sess-diffbox').style.display = s.linediff ? '' : 'none';
    if (s.linediff) $('#sess-diff').innerHTML = diffHtml(s.linediff);
    const body = $('#sess-term .body');
    body.innerHTML = '';
    // type the command, show what was piped in, then the output
    const line = document.createElement('span'); line.className = 'term-line'; line.innerHTML = '<span class="prompt">$</span> <span class="cmd"></span>';
    body.appendChild(line);
    for (const ch of s.cmd) { if (my !== token) return; $('.cmd', line).textContent += ch; await sleep(22); }
    if (s.input) { body.insertAdjacentHTML('beforeend', piped(s.input)); await sleep(300); }
    if (my !== token) return;
    viewer.show(s, { sel: s.sel, level: 1 });
    for (const l of s.out.split('\n')) { if (my !== token) return; body.insertAdjacentHTML('beforeend', `<span class="term-line">${termLine(l)}</span>`); body.scrollTop = body.scrollHeight; await sleep(60); }
    if (s.code) body.insertAdjacentHTML('beforeend', `<span class="term-line o-dim">exit ${s.code}</span>`);
    if (playing) {
      const btn = $$('#sess-steps button')[i], t0 = performance.now(), dur = 5200 + s.say.length * 18;
      while (performance.now() - t0 < dur) { if (my !== token || !playing) return; btn.style.setProperty('--p', (performance.now() - t0) / dur); await sleep(50); }
      if (my === token && playing) { if (i + 1 < ss.length) go(i + 1); else stop(); }
    }
  }
  function stop() { playing = false; $('#sess-play').textContent = '▶ play'; $$('#sess-steps button').forEach(b => b.style.setProperty('--p', 1)); }
  $('#sess-play').onclick = () => { if (playing) return stop(); playing = true; $('#sess-play').textContent = '❚❚ pause'; go(cur + 1 < ss.length ? cur + 1 : 0); };
  $('#sess-next').onclick = () => { stop(); go(cur + 1); };
  $('#sess-prev').onclick = () => { stop(); go(cur - 1); };
  // start when it is first seen
  const o = new IntersectionObserver(es => { if (es[0].isIntersecting) { o.disconnect(); playing = true; $('#sess-play').textContent = '❚❚ pause'; go(0); } }, { threshold: 0.35 });
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

// ───────────────────────── explore + start ─────────────────────────
new Viewer($('#explore-viewer'), 'strata ui · examples/tasks').show(D.tasks, { sel: 'core.run_worker' });
$('#start-cmds').innerHTML = [
  ['cargo build --release', ''], ['alias strata=$PWD/target/release/strata', ''],
  ['strata --file examples/math.strata verify', 'prove what can be proven, fuzz the rest'],
  ['strata init && strata add examples/orders.strata', 'a package: every change checked and logged'],
  ['strata holes', 'what is left to implement'],
  ['strata ui', 'the live viewer, on http://127.0.0.1:7777'],
].map(([c, n]) => cmdLine(c).replace('</span></span>', `</span>${n ? `  <span class="c">// ${esc(n)}</span>` : ''}</span>`)).join('');
