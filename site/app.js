// The presentation. Everything it shows comes from site/data/*.js, recorded by
// site/build.mjs from the real CLI. It is a deck of slides: ← → (or space) step through each slide, then on to the next.
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
  const moved = it.what.map(w => w.match(/^renamed from `([^`]+)`/)).find(Boolean);
  if (moved) {
    const to = it.label.split(' → ').pop();
    return { cls: 'rename', html: `<div class="hl-tag">rename · ${esc(it.kind)}</div><div class="hl-big"><span class="from">${esc(moved[1])}</span> <span class="arrow">→</span> <span class="to">${esc(to)}</span></div>${it.detail.length ? `<div class="hl-why">${it.detail.map(esc).join(' · ')}</div>` : ''}` };
  }
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
    return { cls: 'sig', html: `<div class="hl-tag">signature changed</div><div class="hl-big code">${sig ? segs(sig.segs).replace(/\s*\{\s*$/, '') : esc(it.label)}</div>${it.what.includes('body changed') ? `<div class="hl-why">and its body changed:</div><pre class="hl-body">${codeHtml(it)}</pre>` : ''}` };
  }
  if (it.mk === 'add') return { cls: 'add', html: `<div class="hl-tag ok">added</div><div class="hl-big">${kind} ${esc(it.label)}</div>` };
  return { cls: 'body', html: `<div class="hl-tag">${it.what.map(esc).join(' · ')}</div><div class="hl-big">${kind} ${esc(it.label)}</div>${it.detail.length ? `<div class="hl-why">${it.detail.map(esc).join(' · ')}</div>` : ''}${it.what.includes('body changed') ? `<pre class="hl-body">${codeHtml(it)}</pre>` : ''}` };
}
function structural(c) {
  const items = c.sections.flatMap(s => s.items);
  const ifc = items.filter(i => i.interface_changed);
  // the names an interface change is about: the definition's own, and any new name it gave
  const names = ifc.flatMap(m => [...m.label.split(' → '), ...m.what.flatMap(w => [...w.matchAll(/renamed to `([^`]+)`/g)].map(x => x[1]))]);
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


// ───────────────────────── typing ─────────────────────────
/** Strata code arriving line by line; instant when `fast`. Resolves false if interrupted. */
async function typeCode(el, code, alive, { fast = false, ms = 22, hot = null } = {}) {
  el.innerHTML = '';
  const lines = code.split('\n');
  for (const [i, l] of lines.entries()) {
    if (!alive()) return false;
    const span = document.createElement('span');
    span.className = 'ln' + (hot && hot.test(l) ? ' hot' : '');
    span.innerHTML = hl(l) || ' ';
    if (fast) span.style.animation = 'none';
    el.appendChild(span);
    if (!fast) { if (el.scrollHeight > el.clientHeight) el.scrollTop = el.scrollHeight; await sleep(/^\s*$/.test(l) ? ms * 2 : ms); }
  }
  if (!fast) el.scrollTop = 0;
  return true;
}
/** A command typed into a terminal, then its output; instant when `fast`. */
async function typeTerm(el, s, alive, { fast = false, clear = true } = {}) {
  if (clear) el.innerHTML = '';
  if (fast) { el.insertAdjacentHTML('beforeend', cmdLine(s.cmd) + (s.input ? piped(s.input) : '') + term(s.out)); return true; }
  return typeInto2(el, s, alive);
}
async function typeInto2(body, s, alive) {
  const line = document.createElement('span'); line.className = 'term-line'; line.innerHTML = '<span class="prompt">$</span> <span class="cmd caret"></span>';
  body.appendChild(line);
  const cmd = $('.cmd', line);
  for (const ch of s.cmd) { if (!alive()) return false; cmd.textContent += ch; await sleep(16); }
  cmd.classList.remove('caret');
  if (s.input) { body.insertAdjacentHTML('beforeend', piped(s.input)); await sleep(200); }
  await sleep(220);
  for (const l of s.out.split('\n')) {
    if (!alive()) return false;
    body.insertAdjacentHTML('beforeend', `<span class="term-line ln">${termLine(l)}</span>`);
    body.scrollTop = body.scrollHeight; await sleep(38);
  }
  return true;
}

// ───────────────────────── the codebase as a graph ─────────────────────────
// Each definition is a node, placed in a column by how deep its dependencies go
// (types on the left, what uses them to the right, tests last); an edge is a use.
const KIND_ORDER = { type: 0, trait: 1, fn: 2, impl: 3, test: 4 };
const short = d => {
  const l = d.label || d.name;
  if (d.kind === 'test') { const t = l.replace(/^test\s+/, ''); return t.length > 20 ? t.slice(0, 18) + '…"' : t; }
  return l.length > 24 ? l.slice(0, 23) + '…' : l;
};
const statusOf = d => d.holes ? 'hole' : ({ counterexample: 'fail', proven: 'pass', fuzzed: 'fuzzed', assumed: 'assumed' })[d.verdict] || '';
class Graph {
  constructor(el, { legend = true } = {}) {
    this.el = el; this.nodes = new Map(); this.edges = [];
    el.innerHTML = `<svg></svg>${legend ? `<div class="legend"><span style="--c:var(--ok)">proven</span><span style="--c:var(--warn)">fuzzed / assumed</span><span style="--c:var(--bad)">refuted</span><span style="--c:var(--hole)">hole</span><span style="--c:var(--accent2)">changed</span></div>` : ''}`;
    this.svg = $('svg', el);
    new ResizeObserver(() => { this.place(); this.draw(); }).observe(el);
  }
  /** Put each node at its spot, kept inside the frame and off the others. */
  place() {
    const W = this.el.clientWidth, H = this.el.clientHeight, pad = 14, gap = 6;
    const bs = [...this.nodes].filter(([n]) => this.at?.has(n)).map(([n, el]) => {
      const p = this.at.get(n), w = el.offsetWidth, h = el.offsetHeight;
      return { el, w, h, x: Math.max(w / 2 + pad, Math.min(W - w / 2 - pad, p.x * W)), y: p.y * H };
    });
    // push apart what overlaps, a few rounds
    for (let round = 0; round < 6; round++) {
      let moved = false;
      bs.sort((a, b) => a.y - b.y);
      for (let i = 0; i < bs.length; i++) for (let j = i + 1; j < bs.length; j++) {
        const a = bs[i], b = bs[j];
        if (Math.abs(a.x - b.x) >= (a.w + b.w) / 2 + gap) continue;
        const need = (a.h + b.h) / 2 + gap - (b.y - a.y);
        if (need > 0) { a.y -= need / 2; b.y += need / 2; moved = true; }
      }
      for (const b of bs) b.y = Math.max(b.h / 2 + pad, Math.min(H - b.h / 2 - 40, b.y));
      if (!moved) break;
    }
    for (const b of bs) {
      b.el.style.left = b.x + 'px'; b.el.style.top = b.y + 'px';
      const c = $('.cex', b.el);
      if (c) c.dataset.side = b.x > W * .62 ? 'right' : b.x < W * .3 ? 'left' : '';
    }
  }
  layout(decls) {
    const by = new Map(decls.map(d => [d.name, d])), depth = new Map();
    const dep = n => {
      if (depth.has(n)) return depth.get(n);
      depth.set(n, 0);
      let v = 0;
      for (const x of by.get(n).deps) if (by.has(x)) v = Math.max(v, dep(x) + 1);
      depth.set(n, v); return v;
    };
    decls.forEach(d => dep(d.name));
    const top = Math.max(0, ...decls.filter(d => d.kind !== 'test').map(d => depth.get(d.name)));
    decls.forEach(d => { if (d.kind === 'test') depth.set(d.name, top + 1); });
    const cols = [];
    for (const d of decls) (cols[depth.get(d.name)] ||= []).push(d);
    const at = new Map();
    cols.forEach((col, c) => {
      if (!col) return;
      const key = d => { const ys = d.deps.filter(x => at.has(x)).map(x => at.get(x).y); return ys.length ? ys.reduce((a, b) => a + b) / ys.length : KIND_ORDER[d.kind] ?? 2; };
      col.sort((a, b) => key(a) - key(b) || (KIND_ORDER[a.kind] ?? 2) - (KIND_ORDER[b.kind] ?? 2));
      const n = cols.length, x = n === 1 ? .5 : .1 + c * (.8 / (n - 1));
      col.forEach((d, i) => at.set(d.name, { x, y: .1 + (i + .5) * (.8 / col.length) + (c % 2 ? .02 : -.02) }));
    });
    return at;
  }
  /** Show `decls` (placed as in `opts.layout`, all of them by default) with their statuses. */
  set(decls, { layout = decls, focus = null, changed = new Set(), lit = new Set(), statuses = true, cex = {}, fast = false } = {}) {
    const at = this.at = this.layout(layout), names = new Set(decls.map(d => d.name));
    const slice = focus ? new Set([focus, ...(decls.find(d => d.name === focus)?.deps || []), ...decls.filter(d => d.deps.includes(focus)).map(d => d.name)]) : null;
    for (const [n, el] of this.nodes) if (!names.has(n)) { el.classList.add('gone'); this.nodes.delete(n); setTimeout(() => el.remove(), 500); }
    for (const d of decls) {
      let el = this.nodes.get(d.name);
      const p = at.get(d.name) || { x: .5, y: .5 };
      if (!el) {
        el = document.createElement('div');
        el.innerHTML = `<span class="st"></span><span class="nm"></span><span class="kd"></span>`;
        this.el.appendChild(el); this.nodes.set(d.name, el);
        if (!fast) el.dataset.pop = 1;
      }
      $('.nm', el).textContent = short(d);
      $('.kd', el).textContent = d.kind === 'fn' || d.kind === 'test' ? '' : d.kind;
      el.title = d.label || d.name;
      el.className = ['gnode', 'k-' + d.kind, statuses ? statusOf(d) : '', changed.has(d.name) ? 'changed' : '', focus === d.name ? 'focus' : '', slice && !slice.has(d.name) && !changed.has(d.name) && !d.holes && d.verdict !== 'counterexample' ? 'dim' : '', el.dataset.pop ? 'pop' : ''].filter(Boolean).join(' ');
      delete el.dataset.pop;
      $('.cex', el)?.remove();
      if (cex[d.name]) el.insertAdjacentHTML('beforeend', `<div class="cex">${cex[d.name]}</div>`);
    }
    this.place();
    this.edges = decls.flatMap(d => d.deps.filter(x => names.has(x)).map(x => ({ from: x, to: d.name,
      cls: lit.has(x) || changed.has(x) && changed.has(d.name) ? 'lit' : slice ? (slice.has(x) && slice.has(d.name) && (x === focus || d.name === focus) ? 'focus' : 'dim') : '' })));
    this.animate();
  }
  animate() {
    const t0 = performance.now(), tick = () => { this.draw(); if (performance.now() - t0 < 1100) requestAnimationFrame(tick); };
    requestAnimationFrame(tick);
  }
  draw() {
    const box = this.el.getBoundingClientRect();
    if (!box.width) return;
    this.svg.setAttribute('viewBox', `0 0 ${box.width} ${box.height}`);
    const r = n => { const e = this.nodes.get(n); if (!e) return null; const b = e.getBoundingClientRect(); return { l: b.left - box.left, r: b.right - box.left, y: b.top - box.top + b.height / 2, x: b.left - box.left + b.width / 2 }; };
    this.svg.innerHTML = this.edges.map(e => {
      const a = r(e.from), b = r(e.to);
      if (!a || !b) return '';
      const [x1, x2] = a.x <= b.x ? [a.r, b.l] : [a.l, b.r];
      const mx = (x1 + x2) / 2;
      return `<path class="e ${e.cls}" d="M${x1} ${a.y} C${mx} ${a.y}, ${mx} ${b.y}, ${x2} ${b.y}"/>`;
    }).join('');
  }
  shake() { this.el.classList.remove('shake'); void this.el.offsetWidth; this.el.classList.add('shake'); }
}
const changedBetween = (a, b) => new Set(b.filter(d => { const o = a?.find(x => x.name === d.name); return !o || o.hash !== d.hash; }).map(d => d.name));
const ifaceChanged = (a, b) => new Set(b.filter(d => { const o = a?.find(x => x.name === d.name); return o && o.interface !== d.interface; }).map(d => d.name));

// ───────────────────────── the deck ─────────────────────────
const slides = $$('.slide');
const B = {}; // what each slide does at each of its steps, by id: (k, {dir, fresh, fast, alive}) => …
let cur = -1, sub = 0, token = 0;
const nSteps = i => +slides[i].dataset.steps || 1;
function go(i, k = 0, dir = 1) {
  i = Math.max(0, Math.min(slides.length - 1, i)); k = Math.max(0, Math.min(nSteps(i) - 1, k));
  const fresh = i !== cur;
  if (fresh && cur >= 0) wipe(dir);
  slides.forEach((s, j) => { s.classList.toggle('now', j === i); s.classList.toggle('past', j < i); });
  cur = i; sub = k;
  const my = ++token, alive = () => my === token;
  try { B[slides[i].id]?.(k, { dir, fresh, fast: dir < 0, alive }); } catch (e) { console.error(e); }
  hud();
  history.replaceState(null, '', `#${slides[i].id}${k ? '/' + (k + 1) : ''}`);
  bandsTo(i);
}
const next = () => sub + 1 < nSteps(cur) ? go(cur, sub + 1, 1) : go(cur + 1, 0, 1);
const prev = () => sub > 0 ? go(cur, sub - 1, -1) : cur > 0 && go(cur - 1, nSteps(cur - 1) - 1, -1);
const goTo = id => go(slides.findIndex(s => s.id === id), 0, 1);
function wipe(dir) {
  const w = $('#wipe'); w.className = ''; void w.offsetWidth; w.className = dir < 0 ? 'go back' : 'go';
}
function hud() {
  const s = slides[cur], n = nSteps(cur);
  $('#hud-n').textContent = `${String(cur + 1).padStart(2, '0')} / ${slides.length}`;
  $('#hud-title').textContent = s.dataset.nav;
  $$('#dots button').forEach((b, j) => { b.classList.toggle('on', j === cur); b.classList.toggle('done', j < cur); b.style.setProperty('--p', (sub + 1) / n); });
  $('#hud-sub').innerHTML = n > 1 ? Array.from({ length: n }, (_, j) => `<i class="${j === sub ? 'on' : j < sub ? 'done' : ''}"></i>`).join('') : '';
  $('#hud-prev').disabled = cur === 0 && sub === 0;
  $('#hud-next').disabled = cur === slides.length - 1 && sub === n - 1;
  const total = slides.length;
  $('#progress').style.width = ((cur + (sub + 1) / n) / total) * 100 + '%';
}
$('#dots').innerHTML = slides.map((s, j) => `<button data-j="${j}"><span class="tip">${esc(s.dataset.nav)}</span></button>`).join('');
$('#dots').addEventListener('click', e => { const b = e.target.closest('button'); if (b) go(+b.dataset.j, 0, +b.dataset.j < cur ? -1 : 1); });
$('#hud-prev').onclick = prev; $('#hud-next').onclick = next;
addEventListener('keydown', e => {
  if (e.metaKey || e.ctrlKey || e.altKey || /TEXTAREA|SELECT/.test(e.target.tagName) || (e.target.tagName === 'INPUT' && e.target.type !== 'range')) return;
  const k = e.key;
  if (['ArrowRight', 'ArrowDown', 'PageDown', ' ', 'Enter'].includes(k)) { e.preventDefault(); if (k !== 'Enter' || e.target === document.body) next(); }
  else if (['ArrowLeft', 'ArrowUp', 'PageUp', 'Backspace'].includes(k)) { e.preventDefault(); prev(); }
  else if (k === 'Home') { e.preventDefault(); go(0); }
  else if (k === 'End') { e.preventDefault(); go(slides.length - 1); }
}, true);
// buttons keep focus after a click: let the keys go on driving the deck
addEventListener('click', e => { if (e.target.closest('button')) e.target.closest('button').blur(); });
{ // swipes
  let x0 = null;
  addEventListener('touchstart', e => (x0 = e.touches[0].clientX), { passive: true });
  addEventListener('touchend', e => { if (x0 == null) return; const dx = e.changedTouches[0].clientX - x0; if (Math.abs(dx) > 60) dx < 0 ? next() : prev(); x0 = null; });
}
// a horizontal wheel (trackpad) moves between slides too
{ let last = 0; addEventListener('wheel', e => { if (Math.abs(e.deltaX) < 40 || Math.abs(e.deltaX) < Math.abs(e.deltaY) * 2 || performance.now() - last < 700) return; last = performance.now(); e.deltaX > 0 ? next() : prev(); }, { passive: true }); }

// ───────────────────────── layered bands behind everything, moving with the deck ─────────────────────────
let bandsTo = () => {};
(() => {
  const svg = $('#bands');
  const colors = ['#1a1d2b', '#1d2033', '#22233a', '#2a2540', '#3a2c3c', '#4d3438', '#5e3f33'];
  const W = 1600, paths = colors.map((c, i) => {
    const p = document.createElementNS('http://www.w3.org/2000/svg', 'path');
    p.setAttribute('fill', c); p.setAttribute('opacity', 0.55 + i * 0.05); svg.appendChild(p); return p;
  });
  let t = 0, off = 0, target = 0, rise = 0, riseTo = 0, energy = 0;
  bandsTo = i => { target = i * 1.3; riseTo = i === 0 ? 0 : 230; energy = 1; };
  (function frame() {
    t += 0.0025; off += (target - off) * 0.04; rise += (riseTo - rise) * 0.04; energy *= 0.965;
    paths.forEach((p, i) => {
      const base = 520 + i * 70 + rise * (0.6 + i * 0.12);
      let d = `M0 ${base}`;
      for (let x = 0; x <= W; x += 80) {
        const y = base + Math.sin(x / 260 + t * (1 + i * 0.3) + i + off * (1 + i * .15)) * (18 + i * 4 + energy * 40) + Math.sin(x / 90 + t * 2 + i * 2 + off) * (4 + energy * 10);
        d += ` L${x} ${y.toFixed(1)}`;
      }
      p.setAttribute('d', d + ` L${W} 1000 L0 1000 Z`);
    });
    requestAnimationFrame(frame);
  })();
})();

// ───────────────────────── title ─────────────────────────
$('#title').innerHTML = [...'Strata'].map((c, i) => `<span class="l" style="animation-delay:${0.1 + i * 0.08}s">${c}</span>`).join('');
{
  let typed = false;
  B.hero = async () => {
    if (typed) return; typed = true;
    const el = $('#tagline');
    const parts = [['A language for working with agents. ', ''], ['Code that says what it means, checked and proven as it is written.', 'dim']];
    await sleep(800);
    for (const [t, cls] of parts) {
      const span = document.createElement('span'); if (cls) span.className = cls; el.appendChild(span);
      for (const ch of t) { span.textContent += ch; await sleep(ch === ' ' ? 18 : 26); }
    }
    el.classList.add('done');
  };
}

// ───────────────────────── explanations, arriving sentence by sentence ─────────────────────────
/** Put `html` in `el` a sentence at a time, each after the time it takes to read the one before. */
function say(el, html, fast = false) {
  const parts = html.split(/(?<=[.;!?])\s+(?=[A-Z<"`(])/);
  let t = 0;
  el.innerHTML = parts.map(p => {
    const d = fast ? 0 : t;
    t += Math.min(2400, 450 + p.replace(/<[^>]+>/g, '').length * 26);
    return `<span class="sent" style="--t:${d}ms">${p}</span>`;
  }).join(' ');
}
const sayBlock = (el, title, html, fast, who = '') => {
  el.innerHTML = `${who}<h3>${title}</h3><p></p>`;
  say($('p', el), html, fast);
};

// ───────────────────────── three ideas ─────────────────────────
{
  const g0 = D.session[0].graph, txt = n => g0.find(d => d.name === n).text;
  const verify = D.math.verify.split('\n');
  const pick = (re, extra = 0) => { const i = verify.findIndex(l => re.test(l)); return verify.slice(i, i + 1 + extra); };
  const ideas = [
    { t: 'Code is a tree, not text', say: 'Each definition is stored on its own, as a syntax tree with a content hash. Text is only a view of it. So the tools see what code <b>is</b>: a rename, a new parameter, who uses what. Not lines.',
      code: hl(D.orders.levels[0].trimEnd().split('\n').slice(0, 7).join('\n')) },
    { t: 'Types say what they mean', say: 'A type can carry a condition: <code>Cents</code> is an <code>Int</code> that is never negative. Functions state what they need and what they guarantee, with <code>requires</code> and <code>ensures</code>. Unfinished code is a typed hole.',
      code: hl([txt('Cents'), txt('Quantity'), txt('refund').split('\n').filter(l => !l.startsWith('///')).join('\n')].join('\n\n')) },
    { t: 'Every claim is checked', say: 'An SMT solver proves each contract for every input, or finds the input that breaks it; a fuzzer covers what it can\'t prove. And every edit is checked against the whole codebase before it is stored.',
      code: term([...pick(/^✓ factorial/), ...pick(/^~ isqrt/), ...pick(/^✗ next_digit/, 2)].join('\n')) },
  ];
  $('#ideas-cards').innerHTML = ideas.map((x, i) => `<div class="idea"><div class="n">0${i + 1}</div><h3>${x.t}</h3><p></p><pre>${x.code}</pre></div>`).join('');
  const fixes = [
    ['diffs', 'Diffs are walls of red and green', 'changes you can read'],
    ['proof', 'Tests check the cases someone thought of', 'proof for every input'],
    ['contracts', 'A tighter contract breaks a caller far away', 'every caller re-checked'],
    ['session', 'A broken edit lands, someone finds out later', 'refused, or turned into holes'],
  ];
  $('#ideas-fixes').innerHTML = fixes.map(([to, p, f], i) => `<button data-to="${to}" style="--i:${i}"><span class="p">${p}</span><span class="f">→ ${f}</span></button>`).join('');
  $('#ideas-fixes').addEventListener('click', e => { const b = e.target.closest('button'); if (b) goTo(b.dataset.to); });
  B.ideas = (k, { fast }) => {
    $$('#ideas .idea').forEach((c, j) => {
      const was = c.classList.contains('in');
      c.classList.toggle('in', j <= k); c.classList.toggle('cur', j === k && k < 3);
      if (j <= k && (!was || fast)) say($('p', c), ideas[j].say, fast || j < k);
    });
    $('#ideas-fixes').classList.toggle('in', k === 3);
  };
}

// ───────────────────────── code is a graph of definitions ─────────────────────────
{
  const g0 = D.session[0].graph, graph = new Graph($('#tree-graph'));
  const at = graph.layout(g0);
  const order = [...g0].sort((a, b) => at.get(a.name).x - at.get(b.name).x || at.get(a.name).y - at.get(b.name).y);
  B.tree = async (k, { fast, alive }) => {
    const code = $('#tree-code'), sy = (h, p) => sayBlock($('#tree-say'), h, p, fast);
    if (k === 0) {
      sy('Each definition is stored on its own', 'As a syntax tree with a content hash, not as lines in a file. As each one is written, it takes its place among what it uses and what uses it.');
      $('#tree-title').textContent = 'orders.strata · strata render';
      if (fast) { graph.set(g0, { statuses: false, fast }); code.innerHTML = order.map(d => hl(d.text)).join('\n\n'); return; }
      graph.set([], { layout: g0 }); code.innerHTML = '';
      const shown = [];
      for (const d of order) {
        if (!alive()) return;
        const el = document.createElement('div'); code.appendChild(el);
        if (shown.length) el.style.marginTop = '1.6em';
        for (const l of d.text.split('\n')) {
          if (!alive()) return;
          el.insertAdjacentHTML('beforeend', `<span class="ln">${hl(l) || ' '}</span>`);
          code.scrollTop = code.scrollHeight; await sleep(16);
        }
        shown.push(d);
        graph.set(shown, { layout: g0, statuses: false, changed: new Set([d.name]) });
        await sleep(60);
      }
    } else if (k === 1) {
      sy('Every name is resolved in the tree', 'Selecting <code>Order</code> lights what it uses and what uses it. The tools know these links exactly: they are how renames, re-checks and slices work.');
      $('#tree-title').textContent = '$ strata view slice:Order --level 1';
      graph.set(g0, { statuses: false, focus: 'Order', fast });
      await typeCode(code, D.orders.levels[1].trimEnd(), alive, { fast, ms: 14 });
    } else {
      sy('Every definition carries its status', 'Proven by the solver, fuzzed, or refuted, and holes: unfinished code that still checks. <code>refund</code> is one: a precise task for an agent.');
      $('#tree-title').textContent = '$ strata holes';
      graph.set(g0, { fast });
      code.innerHTML = ''; await typeTerm(code, { cmd: 'strata holes', out: D.orders.holes }, alive, { fast });
    }
  };
}

// ───────────────────────── levels ─────────────────────────
{
  const L = D.orders.levels, lede = $('#levels-say').innerHTML;
  const info = [[0, 'The map', 'one line per definition: what it depends on, and its status'], [1, 'The interfaces', 'types, signatures and contracts: what other code can rely on'], [2, 'The source', 'everything, exactly as stored']];
  $('#lv-pick').innerHTML = info.map(([l, t, d], i) => `<button data-i="${i}"><b>${t} <span class="muted">· level ${l}</span></b><span>${d}</span></button>`).join('');
  $('#lv-pick').addEventListener('click', e => { const b = e.target.closest('button'); if (b) go(cur, +b.dataset.i, +b.dataset.i < sub ? -1 : 1); });
  B.levels = (k, { fast, fresh, alive }) => {
    if (fresh) say($('#levels-say'), lede, fast);
    const l = info[k][0];
    $$('#lv-pick button').forEach((b, j) => b.classList.toggle('on', j === k));
    $('#lv-title').textContent = `$ strata view slice:Order --level ${l}`;
    $('#lv-count').textContent = `${L[l].trimEnd().split('\n').length} lines`;
    return typeCode($('#lv-body'), L[l].trimEnd(), alive, { fast, ms: 10 });
  };
}

// ───────────────────────── structural changes ─────────────────────────
{
  const R = D.rename, renameV = S('A rename is one change'), fill = S('It fills them, and closes the change'), fix = S('It fixes it, and proves it');
  const C = D.contract;
  const big = diffCount(R.diff), bigLines = R.diff.split('\n').length;
  const cases = [
    { tab: 'Rename a type', linediff: R.diff, snap: R, step: R.step, h: 'A rename should read as a rename.',
      say: `It was <code>strata rename Task Job</code>: one type renamed, and the definitions that use it follow. As text, ${big} changed lines across four modules. As a change, one line.` },
    { tab: 'Rename a variant', linediff: renameV.linediff, snap: renameV, step: lastStep(renameV), h: 'The same for a variant.',
      say: `<code>strata rename Shipped Sent</code>. As text, five definitions change, and a line in a test re-wraps because <code>Sent</code> is shorter. What happened is one rename.` },
    { tab: 'Add a parameter', linediff: C.paramDiff, snap: C, step: stepTitled(C, 'discount: members'), h: 'A new parameter, and the calls that follow it.',
      say: `<code>discount</code> takes a new <code>member</code> flag, not used yet. The change is the new parameter; its two call sites only follow it.` },
    { tab: 'Change a signature and its body', linediff: D.orders.couponsDiff, snap: fill, step: stepTitled(fill, 'coupons'), h: 'A new signature, and the body that uses it.',
      say: `<code>total</code> takes a coupon and uses it: the signature and the body both changed, so both are shown. The callers that had to follow are listed, not repeated.` },
    { tab: 'Change only a body', linediff: fix.linediff, snap: fix, step: lastStep(fix), h: 'Only a body changed: nothing else to re-check.',
      say: `The fix to <code>refund</code> changes its body, not its signature or contract, so <b>nothing that uses it needs another look</b>. The verdict before and after is part of the change.` },
  ];
  $('#diff-tabs').innerHTML = cases.map((c, i) => `<button data-i="${i}">${esc(c.tab)}</button>`).join('');
  $('#diff-tabs').addEventListener('click', e => { const b = e.target.closest('button'); if (b) go(cur, +b.dataset.i + 1, +b.dataset.i + 1 < sub ? -1 : 1); });
  const count = async (el, n, unit, alive, ms = 260) => { for (let i = 0; i <= n; i++) { if (!alive()) return; el.textContent = `${i} ${unit}${i === 1 ? '' : 's'}`; await sleep(Math.max(12, ms / n)); } };
  const flip = el => { el.classList.remove('flip'); void el.offsetWidth; el.classList.add('flip'); };
  $('#big-body').innerHTML = diffHtml(R.diff);
  B.diffs = async (k, { fast, alive }) => {
    $('#diffs').classList.toggle('big', k === 0);
    if (k === 0) {
      $('#diff-h').textContent = 'Here is a diff. What happened?'; flip($('#diff-h'));
      say($('#diff-say'), `A change to <code>examples/tasks</code>, as <code>git diff</code> shows it: ${bigLines} lines over four modules. To review it, you read all of them.`, fast);
      const body = $('#big-body');
      body.scrollTop = 0;
      count($('#big-count'), big, 'line', alive, 4000).then(() => alive() && ($('#big-count').textContent = `${big} lines changed`));
      await sleep(fast ? 300 : 1400);
      while (alive()) {
        body.scrollTop += 1.4;
        if (body.scrollTop + body.clientHeight >= body.scrollHeight - 1) { await sleep(1500); if (!alive()) return; body.scrollTop = 0; await sleep(800); }
        await new Promise(requestAnimationFrame);
      }
      return;
    }
    const c = cases[k - 1];
    $('#diff-h').textContent = c.h; flip($('#diff-h'));
    say($('#diff-say'), c.say, fast);
    $$('#diff-tabs button').forEach((b, j) => b.classList.toggle('on', j === k - 1));
    const n = diffCount(c.linediff), st = structural(c.snap.changes[c.step]);
    for (const [id, html] of [['#ld-body', diffHtml(c.linediff)], ['#sd-body', st.html]]) { const el = $(id); el.innerHTML = html; flip(el); }
    $('#ld-count').textContent = `${n} line${n === 1 ? '' : 's'} changed`;
    $('#sd-count').textContent = `${st.count} change${st.count > 1 ? 's' : ''}`;
    $('#sd-title').textContent = `strata diff · ${c.snap.changes.all.steps.find(s => s.id === c.step).title}`;
    if (fast) { $('#ld-n').textContent = `${n} lines`; $('#sd-n').textContent = `${st.count} change${st.count > 1 ? 's' : ''}`; }
    else { count($('#ld-n'), n, 'line', alive); count($('#sd-n'), st.count, 'change', alive); }
  };
}

// ───────────────────────── the refund explorer ─────────────────────────
{
  const fillStep = S('It fills the hole'), testStep = S('The tests pass'), verStep = S('Verification finds the bug'), fixStep = S('It fixes it, and proves it');
  let fixed = false;
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
  range.addEventListener('input', upd);
  const SAY = [
    'An agent fills in a refund policy: what was paid, minus a 5.00 handling fee. The signature already says what must hold, for <b>every</b> order: <code>ensures it &gt;= 0</code>. Drag the order value.',
    'It writes a test, as a careful agent would: a 9.00 order, refunded 4.00. <b>The test passes.</b>',
    '<code>strata verify</code> checks the contract for every order, not only the ones someone thought of. An order worth less than the fee gets a <b>negative refund</b>, and the input comes with the verdict.',
    'Fixed, then verified: <b>proven for every order</b>. Drag the value: nothing goes below zero.',
  ];
  $('#ex-test').onclick = () => go(cur, 1, 1);
  $('#ex-verify').onclick = () => go(cur, 2, 1);
  $('#ex-fix').onclick = () => go(cur, 3, 1);
  B.proof = async (k, { fast, alive }) => {
    say($('#proof-say'), SAY[k], fast);
    fixed = k >= 3;
    $('#ex-code').innerHTML = hl((fixed ? fixStep : fillStep).input);
    $('#ex-fix').disabled = k < 2;
    axis.classList.toggle('scanned', k === 2); axis.classList.remove('scanning');
    $('#pin-cex').style.opacity = k === 2 ? '' : 0;
    $('.danger', axis).style.width = k === 2 ? '' : '0';
    range.value = k === 2 ? 63 : k === 3 ? 300 : 900; upd();
    const el = $('#ex-term');
    if (k === 0) { el.innerHTML = '<span class="o-dim">Run the tests (→), then verify.</span>'; return; }
    if (k === 1) return typeTerm(el, testStep, alive, { fast });
    if (k === 2) {
      if (fast) return typeTerm(el, verStep, alive, { fast });
      axis.classList.remove('scanned'); $('.danger', axis).style.width = '0'; range.value = 900; upd();
      void axis.offsetWidth; axis.classList.add('scanning');
      el.innerHTML = cmdLine(verStep.cmd);
      await sleep(1300); if (!alive()) return;
      $('.danger', axis).style.width = ''; axis.classList.add('scanned', 'flash'); setTimeout(() => axis.classList.remove('flash'), 900);
      range.value = 63; upd();
      for (const l of verStep.out.split('\n')) { if (!alive()) return; el.insertAdjacentHTML('beforeend', `<span class="term-line ln">${termLine(l)}</span>`); await sleep(70); }
      return;
    }
    return typeTerm(el, { ...fixStep, input: null }, alive, { fast });
  };
}

// ───────────────────────── the verifier's verdicts, on math.strata ─────────────────────────
{
  const G = D.math.graph, graph = new Graph($('#math-graph'));
  const lines = D.math.verify.split('\n');
  const verdictOf = l => { const m = l.match(/^([✓✗~≈○])\s+(\S+)/); return m && { mark: m[1], name: m[2] }; };
  const cex = {};
  for (const d of G) if (d.verdict === 'counterexample') cex[d.name] = `${esc(d.name)}(${(d.args || []).map(a => esc(JSON.stringify(a))).join(', ')}) → ${esc((d.message || '').replace(/\s*\(stack:.*$/, ''))}`;
  B.verdicts = async (k, { fast, alive }) => {
    const el = $('#math-verify');
    if (k === 1) {
      say($('#verdicts-say'), 'No test was written for <code>next_digit</code> or <code>average</code>. Their types and contracts were enough to find the input that breaks each.', fast);
      graph.set(G, { cex, fast });
      el.innerHTML = term(D.math.verify);
      return;
    }
    say($('#verdicts-say'), 'Each verdict says how sure it is: <span class="o-ok">✓ proven</span> for every input, <span class="o-warn">~ fuzzed</span> where the solver couldn\'t prove it, <span class="o-bad">✗ refuted</span> with the input that breaks it. Termination too: nobody knows whether Collatz ends.', fast);
    if (fast) { graph.set(G, { fast }); el.innerHTML = term(D.math.verify); return; }
    const seen = new Set();
    graph.set(G.map(d => ({ ...d, verdict: null })), { fast: true });
    el.innerHTML = '';
    await sleep(500);
    for (const l of lines) {
      if (!alive()) return;
      el.insertAdjacentHTML('beforeend', `<span class="term-line ln">${termLine(l)}</span>`); el.scrollTop = el.scrollHeight;
      const v = verdictOf(l);
      if (v && G.some(d => d.name === v.name)) {
        seen.add(v.name);
        graph.set(G.map(d => seen.has(d.name) ? d : { ...d, verdict: null }), { changed: new Set([v.name]), fast: true });
        await sleep(130);
      } else await sleep(30);
    }
  };
}

// ───────────────────────── contracts: the problem, then a walk through an edit ─────────────────────────
{
  const C = D.contract, g = $('#cgraph');
  const nodes = {
    discount: { x: 22, y: 50, t: 'discount', s: 'the contract changes here' },
    checkout: { x: 74, y: 24, t: 'checkout', s: 'calls discount(…)' },
    totals: { x: 74, y: 76, t: 'test "totals"', s: 'calls discount(20000)' },
  };
  const LABEL = { pass: '✓ proven', fail: '✗ counterexample', changed: 'changed', recheck: 're-check', hole: '?hole', unknown: '?' };
  g.innerHTML = `<svg><path id="e1"/><path id="e2"/></svg>` + Object.entries(nodes).map(([k, n]) => `<div class="node" id="n-${k}" style="left:${n.x}%;top:${n.y}%"><span class="badge"></span>${esc(n.t)}<small>${esc(n.s)}</small></div>`).join('');
  function edges() {
    const W = g.clientWidth, H = g.clientHeight, p = n => [nodes[n].x / 100 * W, nodes[n].y / 100 * H];
    const e = (a, b) => { const [x1, y1] = p(a), [x2, y2] = p(b); return `M${x1 + 90} ${y1} C${(x1 + x2) / 2} ${y1}, ${(x1 + x2) / 2} ${y2}, ${x2 - 100} ${y2}`; };
    $('#e1').setAttribute('d', e('discount', 'checkout')); $('#e2').setAttribute('d', e('discount', 'totals'));
  }
  new ResizeObserver(edges).observe(g);
  const tighten = C.steps.find(s => s.title === 'Tighten discount’s contract');
  const PROBLEM = { title: 'Tighten a function, break a caller far away', g: { discount: 'changed', checkout: 'unknown', totals: 'unknown' },
    say: 'A stricter precondition is one added line in a diff. Nothing in it says which callers relied on the old contract, or which of them now break. Let\'s walk through an edit, and see how contracts make this powerful for an agent.' };
  B.contracts = async (k, { fast, alive }) => {
    const s = k ? C.steps[k - 1] : PROBLEM;
    for (const n of Object.keys(nodes)) {
      const v = s.g[n] || '', el = $('#n-' + n), was = el.className;
      el.className = 'node ' + v;
      if (!fast && was !== el.className) { el.style.animation = 'none'; void el.offsetWidth; el.style.animation = 'nodeping .7s'; }
      $('#n-' + n + ' .badge').textContent = LABEL[v] || '';
    }
    $$('#cgraph path').forEach(p => { p.classList.toggle('lit', !!s.g.lit); p.style.stroke = ''; });
    if (s.g.bad) $('#e1').style.stroke = 'var(--bad)';
    sayBlock($('#c-say'), esc(s.title), k ? md(s.say) : s.say, fast, k ? `<div class="who"><i></i>step ${k} of ${C.steps.length}</div>` : '<div class="who you"><i></i>the problem</div>');
    const card = $('#c-code');
    if (!k) {
      card.classList.remove('on');
      $('#c-term-title').textContent = 'git diff';
      $('#c-term').innerHTML = diffHtml(tighten.linediff);
      return;
    }
    $('#c-term-title').textContent = '~/shop';
    card.classList.toggle('on', !!s.input);
    const typing = s.input ? typeCode($('.body', card), s.input, alive, { fast, ms: 18 }) : null;
    await typeTerm($('#c-term'), { ...s, input: null }, alive, { fast });
    await typing;
  };
}

// ───────────────────────── an agent's session: holes and refusals ─────────────────────────
{
  const ss = D.session, graph = new Graph($('#sess-graph'));
  // one layout for the whole session, so nothing moves but what changed
  const every = [...new Map(ss.flatMap(s => s.graph).map(d => [d.name, d])).values()];
  B.session = async (k, { fast, alive }) => {
    const stamp = $('#sess-stamp'), card = $('#sess-code');
    stamp.className = 'stamp'; stamp.textContent = '';
    if (k === 0) {
      sayBlock($('#sess-say'), 'A broken edit lands, and someone finds out later',
        'Text accepts anything: the breakage shows up in CI, in review, or in production. In Strata, an edit that doesn\'t check is refused, and one that breaks what uses it turns those uses into holes: typed tasks. Let\'s follow an agent through a session.', fast, '<div class="who you"><i></i>the problem</div>');
      graph.set(ss[0].graph, { layout: every, fast });
      card.classList.remove('on');
      $('#sess-term').innerHTML = '<span class="term-line"><span class="prompt">$</span> <span class="cmd caret"></span></span>';
      return;
    }
    const s = ss[k - 1], before = ss[k - 2]?.graph;
    const who = s.who === 'you' ? 'you' : s.who === 'agent B' ? 'agentb' : '';
    sayBlock($('#sess-say'), esc(s.title), md(s.say), fast, `<div class="who ${who}"><i></i>${esc(s.who)}</div>`);
    const refused = s.code && /refused/.test(s.title);
    const cex = {};
    for (const d of s.graph) if (d.verdict === 'counterexample') cex[d.name] = `${esc(d.name)}(${(d.args || []).map(a => esc(JSON.stringify(a))).join(', ')}): ${esc((d.message || '').replace(/\s*\(stack:.*$/, ''))}`;
    graph.set(s.graph, { layout: every, focus: s.sel, changed: before ? changedBetween(before, s.graph) : new Set(), lit: before ? ifaceChanged(before, s.graph) : new Set(), cex, fast });
    card.classList.toggle('on', !!s.input);
    const typing = s.input ? typeCode($('.body', card), s.input, alive, { fast, ms: 20 }) : Promise.resolve(true);
    const done = await typeTerm($('#sess-term'), { ...s, input: null }, alive, { fast });
    await typing;
    if (!done || !alive()) return;
    if (s.code) $('#sess-term').insertAdjacentHTML('beforeend', `<span class="term-line o-dim">exit ${s.code}</span>`);
    if (refused) { graph.shake(); stamp.textContent = 'rejected'; stamp.classList.add('bad'); void stamp.offsetWidth; stamp.classList.add('on'); }
    else if (/merge/.test(s.title)) { stamp.textContent = 'merged · checked'; stamp.classList.add('ok'); void stamp.offsetWidth; stamp.classList.add('on'); }
    else if (s.input) { await sleep(fast ? 0 : 1400); if (alive()) card.classList.remove('on'); }
  };
}

// ───────────────────────── merge ─────────────────────────
{
  const M = D.orders.merge;
  $('#lanes').innerHTML = `<svg viewBox="0 0 1200 190" preserveAspectRatio="xMidYMid meet">
    <text class="lane-label" x="0" y="54">main</text><text class="lane-label" x="0" y="144">agent-b</text>
    <path d="M90 50 L1130 50" stroke="#8ea2ff" stroke-width="2.5" fill="none" class="draw" data-k="0"/>
    <path d="M250 50 C 330 50, 330 140, 410 140 L 820 140" stroke="#63e6be" stroke-width="2.5" fill="none" class="draw" data-k="1"/>
    <path d="M820 140 C 900 140, 900 50, 980 50" stroke="#63e6be" stroke-width="2.5" fill="none" class="draw" data-k="2"/>
    <g class="commit anim" data-k="0"><circle cx="250" cy="50" r="9" stroke="#8ea2ff"/><text x="232" y="28">shared</text></g>
    <g class="commit anim" data-k="0"><circle cx="600" cy="50" r="9" stroke="#8ea2ff"/><text x="520" y="28">A: rename line_total → line_cost</text></g>
    <g class="commit anim" data-k="1"><circle cx="600" cy="140" r="9" stroke="#63e6be"/><text x="520" y="174">B: rewrite line_total's body</text></g>
    <g class="commit anim" data-k="2"><circle cx="980" cy="50" r="12" stroke="#5fd07a" style="fill:#5fd07a33"/><text x="935" y="28" style="fill:#5fd07a">✓ merged, checked</text></g>
  </svg>`;
  const SAY = [
    'On <code>main</code>, agent A renames <code>line_total</code> to <code>line_cost</code>.',
    'On a branch, agent B rewrites the body of <code>line_total</code>. As text, the two changes overlap.',
    'Strata\'s merge driver pairs definitions, applies A\'s rename to B\'s code, and checks the result before committing it.',
  ];
  B.merge = async (k, { fast, alive }) => {
    say($('#merge-say'), SAY[k], fast);
    $$('#lanes [data-k]').forEach(e => e.classList.toggle('in', +e.dataset.k <= k));
    $$('#merge .three .pane').forEach(p => p.classList.toggle('in', +p.dataset.k <= k));
    const panes = [['#m-a', { cmd: M.a.cmd, out: M.a.out }], ['#m-b', { cmd: M.b.cmd, input: M.b.input, out: M.b.out }], ['#m-sync', { cmd: 'strata sync main', out: M.sync }]];
    for (const [j, [id, s]] of panes.entries()) {
      if (j > k) { $(id).innerHTML = ''; continue; }
      if (j < k || fast) { $(id).innerHTML = cmdLine(s.cmd) + (s.input ? piped(s.input) : '') + term(s.out); continue; }
      await typeTerm($(id), s, alive);
    }
    if (k === 2 && alive()) $('#m-sync').insertAdjacentHTML('beforeend', `<span class="term-line o-dim">\n// the merged result</span>` + hl(M.result));
  };
}

// ───────────────────────── more of the language ─────────────────────────
{
  const tiles = [
    ['f-capabilities', 'Capabilities', 'What a program may touch, in its signature.'],
    ['f-mocks', 'Mocks', 'Fakes for tests, declared as types.'],
    ['f-derive', 'Derived code', 'Code written by code, while compiling.'],
    ['f-iterators', 'Iterators', 'Lazy, possibly endless, and honest about it.'],
    ['f-native', 'comptime and C', 'Run code while compiling; call C with contracts.'],
  ];
  $('#tiles').innerHTML = tiles.map(([to, t, d], i) => `<button data-to="${to}" style="--i:${i}"><b>${t}</b><span>${d}</span></button>`).join('');
  $('#tiles').addEventListener('click', e => { const b = e.target.closest('button'); if (b) goTo(b.dataset.to); });
}
const F = D.features;
/** A feature, briefly: the explanation on the side, arriving as it is read; the code and what the tools said beside it. */
function feature(id, { title, steps }) {
  const el = $('#' + id);
  el.innerHTML = `<div class="split"><div class="col side-text"><div class="kicker fx">More of the language</div><h2 class="fx small">${title}</h2><div class="say"></div></div>
    <div class="col"><div class="pane code-pane fx"><div class="bar"><span class="dots"><i></i><i></i><i></i></span><span class="title"></span></div><div class="body"></div></div><div class="outs"></div></div></div>`;
  let shown = null;
  B[id] = async (k, { fast, alive }) => {
    const s = steps[k];
    const sy = $('.say', el);
    sy.innerHTML = `<p class="lede"></p>${s.points ? `<ul class="points">${s.points.map(p => `<li>${p}</li>`).join('')}</ul>` : ''}`;
    say($('p', sy), s.say, fast);
    $('.code-pane .title', el).textContent = s.file;
    let typed = Promise.resolve();
    if (shown !== s.code || fast) { shown = s.code; typed = typeCode($('.code-pane .body', el), s.code, alive, { fast, ms: 22, hot: s.hot }); }
    const outs = $('.outs', el); outs.innerHTML = '';
    await typed;
    for (const o of s.outs || []) {
      if (!alive()) return;
      outs.insertAdjacentHTML('beforeend', `<div class="pane term ${o.code ? 'bad' : ''}"><div class="bar"><span class="dots"><i></i><i></i><i></i></span><span class="title">${esc(s.where || '~')}</span>${o.code ? `<span class="count bad">exit ${o.code}</span>` : ''}</div><div class="body wrap"></div></div>`);
      const body = $$('.outs .body', el).at(-1);
      if (fast) { typeTerm(body, o, alive, { fast: true }); continue; }
      await sleep(200);
      await typeTerm(body, o, alive);
      if (o.code && alive()) body.closest('.pane').classList.add('shake');
    }
  };
}
feature('f-capabilities', {
  title: 'What is a capability?',
  steps: [
    { file: 'examples/wc', code: F.capabilities.main + '\n\n' + F.capabilities.wc, hot: /Console|Disk|Files/,
      say: 'A capability is the permission to reach the outside world: the terminal, the disk, a database. It holds no data and has no constructor: only the runtime makes one, for <code>main</code>. So a program\'s signature lists everything it can touch. <code>main</code> hands them on as traits, <code>Console</code> and <code>Files</code>, which anything may implement.' },
    { file: 'examples/wc · a test', code: F.capabilities.test, where: '~/wc', outs: [F.capabilities.test_out, F.capabilities.run_out],
      say: 'So tests hand in stand-ins: a console that records, files in memory. <code>strata test</code> never touches the terminal or the disk. Under <code>strata run</code>, the same code gets the real ones.' },
  ],
});
feature('f-mocks', {
  title: 'Mocks: fakes for tests',
  steps: [
    { file: 'mocks', code: F.mocks.src, hot: /^mock|^impl Files for MemDisk/, where: '~/mocks', outs: [F.mocks.test_out],
      say: 'A fake is declared like a type, with <code>mock</code> in place of <code>type</code>, and implemented like any type. It may leave methods out: <code>Files</code> has seven, this one writes two.' },
    { file: 'mocks', code: F.mocks.src, hot: /^mock|^impl Files for MemDisk/, where: '~/mocks', outs: [F.mocks.verify_out],
      say: 'The methods it left out are generated: they answer within the method\'s types and contracts. <code>verify</code> reports a mock as assumed, since the tests that use it check it.' },
    { file: 'mocks', code: F.mocks.src, where: '~/mocks', outs: [F.mocks.refused],
      say: 'A fake never reaches a real run: using one outside a test is refused, and the codebase is left as it was.' },
  ],
});
feature('f-derive', {
  title: 'Code that writes code',
  steps: [
    { file: 'examples/hello', code: F.derive.options, hot: /^derive/, where: '~/hello', outs: [F.derive.help],
      say: '<code>derive CommandLine for Options</code> generates a command-line parser from the type: its fields, doc comments, defaults and refinements. Derives are written in Strata, run while compiling, and give ordinary <code>impl</code>s.' },
    { file: 'examples/hello', code: F.derive.options, hot: /times/, where: '~/hello', outs: [F.derive.run_out, F.derive.bad],
      say: 'The options arrive in <code>main</code> already checked. A value outside a refinement is refused at the boundary, and the message says which condition.' },
    { file: 'signup', code: F.derive.json, hot: /^derive|where/, where: '~/signup', outs: [F.derive.json_run],
      say: '<code>derive Json</code> works the same way: reading checks every refinement, and an error says where it broke.' },
  ],
});
feature('f-iterators', {
  title: 'Lazy, possibly endless',
  steps: [
    { file: 'naturals', code: F.iterators.src, hot: /Iterator|filter/, where: '~/naturals', outs: [F.iterators.run_out],
      say: 'An iterator is a value walked once, lazily: a trait with <code>next(mut self)</code>. <code>map</code> and <code>filter</code> are lazy too, so they work on endless iterators.' },
    { file: 'naturals', code: F.iterators.src, hot: /for s in/, where: '~/naturals', outs: [F.iterators.verify_out],
      say: '<code>verify</code> shows that a loop ends over a list or a <code>FiniteIterator</code>. Over this endless one it can\'t, and says so instead of guessing.' },
  ],
});
feature('f-native', {
  title: 'Compile time, and C',
  steps: [
    { file: 'comptime', code: F.native.comptime, hot: /comptime/, where: '~/comptime', outs: [F.native.comptime_run],
      say: '<code>comptime e</code> is evaluated while compiling. The program carries only the result, and the value is cached between runs.' },
    { file: 'examples/fuzzy · module text', code: F.native.externs, hot: /^extern c/, where: '~/fuzzy', outs: [F.native.fuzzy_run],
      say: '<code>extern c</code> declares a C function: a signature with contracts, and no body. Arguments and results are checked at the boundary, both ways, and only the module that declares it may call it.' },
  ],
});

// ───────────────────────── under the hood ─────────────────────────
{
  const facts = [
    ['LLVM', 'One code generator. <code>run</code>, <code>test</code> and <code>verify</code> compile on the JIT, <code>strata build</code> makes executables.'],
    ['RC', 'Memory by reference counting: a value is freed when nothing holds it, and updated in place when only one thing does.'],
    ['std', 'Written in Strata, precompiled. Rust is only the native I/O layer.'],
    ['∞', 'No limits on loops or recursion: <code>Int</code> grows into a big integer, and running out of stack is a fault that says where.'],
  ];
  $('#facts').innerHTML = facts.map(([b, t], i) => `<div style="transition-delay:${i * 90}ms"><b>${b}</b><span>${t}</span></div>`).join('');
  B.engine = async (k, { fast, alive }) => {
    $$('#facts div').forEach(d => d.classList.add('in'));
    await typeCode($('#std-code'), F.std.src, alive, { fast: fast || k === 1, ms: 30 });
    if (k === 0) { $('#std-term').innerHTML = '<span class="o-dim">→ run it</span>'; return; }
    await typeTerm($('#std-term'), F.std.run_out, alive, { fast });
    if (alive()) $('#std-term').insertAdjacentHTML('beforeend', '<span class="term-line o-dim">\n// the same tree as Rust\'s regex-syntax: tests/regex.rs\n// checks the two agree on curated and random patterns</span>');
  };
}

$('#start-cmds').innerHTML = [
  ['cargo build --release', ''], ['alias strata=$PWD/target/release/strata', ''],
  ['strata --file examples/math.strata verify', 'prove what can be proven, fuzz the rest'],
  ['strata run examples/wc -- README.md', 'a program, compiled on the JIT'],
  ['strata init && strata add examples/orders.strata', 'a package: every change checked and logged'],
  ['strata holes', 'what is left to implement'],
  ['strata ui', 'the live viewer, on http://127.0.0.1:7777'],
].map(([c, n]) => cmdLine(c).replace('</span></span>', `</span>${n ? `  <span class="c">// ${esc(n)}</span>` : ''}</span>`)).join('');

// ───────────────────────── start: from the address, if it names a slide ─────────────────────────
{
  const [id, k] = location.hash.slice(1).split('/');
  const i = slides.findIndex(s => s.id === id);
  go(i < 0 ? 0 : i, k ? +k - 1 : 0, 1);
}
