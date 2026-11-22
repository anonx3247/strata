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
  const vs = (c.verification || []).filter(v => !/not verified yet/.test(v.after));
  const ver = vs.length ? `<div class="hl-ver">${vs.map(v => `<span><code>${esc(v.name)}</code> ${esc(v.before)} → <b class="${/counter|fail/.test(v.after) ? 'o-bad' : /proven/.test(v.after) ? 'o-ok' : ''}">${esc(v.after)}</b></span>`).join('')}</div>` : '';
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
  // the first slide of a section: its title first, then out of the way
  const section = fresh && dir > 0 && k === 0 ? slides[i].dataset.section : null;
  slides[i].classList.toggle('intro', !!section);
  const run = () => { try { B[slides[i].id]?.(k, { dir, fresh, fast: dir < 0, alive }); } catch (e) { console.error(e); } };
  const sect = $('#sect');
  sect.className = '';
  if (section) {
    $('span', sect).textContent = section; void sect.offsetWidth; sect.className = 'on';
    setTimeout(() => { if (alive()) run(); }, 1500);
  } else run();
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

const V = D.verify, F = D.features;

// ───────────────────────── how an agent works today (an illustration) ─────────────────────────
{
  const FILES = ['orders/models.py', 'orders/service.py', 'billing/discount.py', 'billing/refund.py', 'billing/legacy.py', 'api/routes.py',
    'api/schemas.py', 'db/repository.py', 'utils/money.py', 'tests/test_orders.py', 'tests/test_discount.py', 'tests/test_refund.py'];
  const rnd = (i, j) => { const x = Math.sin(i * 91.7 + j * 13.3) * 43758.5; return x - Math.floor(x); };
  const box = $('#files');
  box.innerHTML = FILES.map((f, i) => `<div class="file" data-i="${i}"><div class="fname">${f}</div><div class="lines">${Array.from({ length: 15 }, (_, j) => `<i style="width:${18 + rnd(i, j) * 72}%;margin-left:${(rnd(j, i) < .4 ? 1 : 0) * 8}%"></i>`).join('')}</div></div>`).join('') + '<div class="agent" id="agent"><i></i><span>agent</span></div>';
  const file = i => $$('.file', box)[i], bar = (i, j) => $$('.lines i', file(i))[j % 15];
  const agent = $('#agent');
  const moveTo = i => { const f = file(i); agent.style.left = f.offsetLeft + f.offsetWidth / 2 + 'px'; agent.style.top = f.offsetTop + f.offsetHeight * .45 + 'px'; $$('.file', box).forEach(x => x.classList.toggle('here', x === f)); };
  const mark = (i, j, cls) => { const b = bar(i, j); if (b) { b.className = cls; } };
  const reset = () => { $$('.lines i', box).forEach(b => (b.className = '')); $$('.file', box).forEach(f => f.classList.remove('here', 'grow')); $$('.lines i.extra', box).forEach(b => b.remove()); };
  const T = $('#today-term');
  const line = async (cmd, outs, alive, fast, ms = 220) => {
    if (!alive()) return false;
    T.insertAdjacentHTML('beforeend', cmd ? cmdLine(cmd) : '');
    for (const o of outs) { T.insertAdjacentHTML('beforeend', `<span class="term-line ln">${termLine(o)}</span>`); }
    T.scrollTop = T.scrollHeight;
    if (!fast) await sleep(ms);
    return alive();
  };
  const STEPS = [
    { h: 'What does an agent do with a coding task? First, it finds its way around.', say: 'It builds a map of the codebase in its head: where each declaration lives, which files it is in, what uses it. It gets there by reading files and searching them, again and again.',
      async run(a, fast) {
        const steps = [
          ['grep -rn "class Order" .', ['orders/models.py:12: class Order(BaseModel):', 'api/schemas.py:40: class OrderOut(BaseModel):'], [[0, 3], [6, 8]]],
          ['grep -rn "def discount" .', ['billing/discount.py:8: def discount(amount):', 'billing/legacy.py:41: def discount(amount, rate=0.1):'], [[2, 2], [4, 9]]],
          ['cat billing/discount.py', ['(54 lines)'], [[2, 0], [2, 4], [2, 7]]],
          ['grep -rn "discount(" .', ['orders/service.py:88', 'api/routes.py:23', 'billing/refund.py:30', 'tests/test_discount.py:5', 'tests/test_orders.py:61', '5 more…'], [[1, 11], [5, 4], [3, 6], [10, 2], [9, 13]]],
          ['cat orders/service.py', ['(212 lines)'], [[1, 1], [1, 6], [1, 11]]],
        ];
        for (const [cmd, outs, hits] of steps) {
          for (const [f, l] of hits) { moveTo(f); mark(f, l, 'hit'); if (!fast) await sleep(260); if (!a()) return; }
          if (!await line(cmd, outs, a, fast)) return;
        }
      } },
    { h: 'Then it edits lines of text, in several files.', say: 'It changes lines in four files at once, and deals with syntax and formatting on the way. The checks run when it thinks of running them.',
      async run(a, fast) {
        const edits = [[2, [3, 4, 5], 'billing/discount.py  +4 −1'], [1, [11, 12], 'orders/service.py  +2 −2'], [5, [4], 'api/routes.py  +1 −1'], [6, [8, 9], 'api/schemas.py  +3 −0']];
        for (const [f, ls, msg] of edits) { moveTo(f); ls.forEach(l => mark(f, l, 'edit')); if (!await line(null, [`edited ${msg}`], a, fast, 380)) return; }
        if (!await line('python -m pytest -q', ['error: IndentationError: unexpected indent (service.py, line 90)'], a, fast, 500)) return;
        moveTo(1); mark(1, 13, 'bad'); if (!fast) await sleep(300);
        mark(1, 13, 'edit'); await line(null, ['edited orders/service.py  +1 −1'], a, fast);
      } },
    { h: 'Then it finds out what broke.', say: 'The change to one function broke code in files the agent never opened. It runs the checks, reads the errors, searches for the callers again, and fixes them one by one. It can\'t finish one thing before moving to the next.',
      async run(a, fast) {
        if (!await line('mypy . && pytest -q', [], a, fast, 300)) return;
        const errs = [[5, 4], [6, 9], [3, 6], [9, 13], [10, 2], [10, 7], [11, 3], [7, 5], [0, 10], [8, 2], [3, 12], [11, 9]];
        let n = 0;
        for (const [f, l] of errs) { mark(f, l, 'bad'); n += 3; $('#today-count').textContent = `${n} errors`; if (!fast) await sleep(90); if (!a()) return; }
        await line(null, ['error: Missing positional argument "member" in call to "discount"', 'error: Argument 1 has incompatible type "Decimal"', '…', 'Found 37 errors in 9 files'], a, fast, 500);
        $('#today-count').textContent = '37 errors';
        for (const [f, l] of errs.slice(0, 6)) { moveTo(f); if (!fast) await sleep(320); if (!a()) return; mark(f, l, 'edit'); }
        await line('grep -rn "discount(" .', ['11 matches in 8 files'], a, fast);
      } },
    { h: 'And it writes tests. Many of them.', say: 'To be sure nothing broke, it writes tests: hundreds, many of them obvious, all kept apart from the code they check. An assumption nobody wrote a test for isn\'t checked at all, and the next change can break it without anyone noticing.',
      async run(a, fast) {
        $('#today-count').textContent = '';
        const names = ['test_discount_zero', 'test_discount_one_euro', 'test_discount_rounding', 'test_discount_negative', 'test_member_discount', 'test_refund_fee', 'test_refund_cancelled', 'test_refund_shipped', 'test_order_total_empty', 'test_order_total_two_lines'];
        let n = 186;
        for (const [k, nm] of names.entries()) {
          const f = 9 + (k % 3); moveTo(f);
          const lines = $('.lines', file(f)); lines.insertAdjacentHTML('beforeend', `<i class="extra edit" style="width:${30 + rnd(k, 3) * 60}%"></i>`);
          n += 3; $('#today-count').textContent = `${n} tests`;
          if (!await line(null, [`+ ${nm}`], a, fast, 160)) return;
        }
        $('#today-count').textContent = '214 tests';
        mark(3, 9, 'bad'); await line(null, ['', '# refund() assumes total >= fee: no test says so'], a, fast);
      } },
    { h: 'And then someone has to review it.', say: 'The contracts the code relies on are spread over files, comments and tests. The diff a person is asked to review runs to thousands of lines across dozens of files, and it rarely gets read closely.',
      async run(a, fast) {
        $('#today-count').textContent = '';
        agent.classList.add('away');
        await line('git diff --stat', [], a, fast, 300);
        for (let i = 0; i < FILES.length; i++) {
          for (let j = 0; j < 15; j++) if (rnd(i + 3, j) < .35) mark(i, j, rnd(j, i + 5) < .55 ? 'add' : 'bad');
          if (!fast) await sleep(70); if (!a()) return;
        }
        await line(null, [' 23 files changed, 1842 insertions(+), 967 deletions(-)'], a, fast);
        $('#today-count').textContent = '+1842 −967';
      } },
  ];
  B.today = async (k, { fast, alive }) => {
    const s = STEPS[k];
    $('#today-h').textContent = s.h;
    $('#today-say').innerHTML = '<p></p>'; say($('#today-say p'), s.say, fast);
    agent.classList.remove('away');
    // each step starts from what the earlier ones left, replayed at once
    reset(); T.innerHTML = ''; $('#today-count').textContent = '';
    for (let j = 0; j < k; j++) await STEPS[j].run(() => alive(), true);
    if (!alive()) return;
    T.innerHTML = '';
    await sleep(fast ? 0 : 400);
    await s.run(alive, fast);
  };
}

// ───────────────────────── three ideas ─────────────────────────
{
  const g0 = D.session[0].graph;
  const verify = D.math.verify.split('\n');
  const pick = (re, extra = 0) => { const i = verify.findIndex(l => re.test(l)); return verify.slice(i, i + 1 + extra); };
  const sig = V.contracts.discount.split('\n').filter(l => !l.startsWith('///')).join('\n').replace(/\s*\{[\s\S]*$/, '');
  const ideas = [
    { t: 'Types as contracts', say: 'Types carry constraints, and functions have preconditions and postconditions. Both are part of the signature, and both are checked wherever the code is used.',
      code: hl(V.contracts.types.split('\n').slice(0, 2).join('\n') + '\n\n' + sig) },
    { t: 'Proving over testing', say: 'An SMT solver proves each contract for every input, or finds an input that breaks it, and a fuzzer covers what it can\'t prove. Most tests become a line in a signature.',
      code: term([...pick(/^✓ factorial/), ...pick(/^~ isqrt/), ...pick(/^✗ next_digit/, 2)].join('\n')) },
    { t: 'The code is the AST', say: 'Code is stored as its syntax tree instead of text files. Text is rendered when someone wants to read it, and edits are parsed back into the tree, so changes, views and checks work on definitions instead of lines.',
      code: hl(D.orders.levels[0].trimEnd().split('\n').slice(0, 7).join('\n')) },
  ];
  $('#ideas-cards').innerHTML = ideas.map((x, i) => `<div class="idea"><div class="n">0${i + 1}</div><h3>${x.t}</h3><p></p><pre>${x.code}</pre></div>`).join('');
  const fixes = [
    ['views', 'It searches files to find its way', 'a view of just what it needs'],
    ['contracts', 'Hundreds of tests, apart from the code', 'contracts, proven or fuzzed'],
    ['travel', 'A change breaks code far away', 'changes follow the graph'],
    ['diffs', 'Diffs too long to read', 'changes you can read'],
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

// ───────────────────────── slides with the explanation beside the code ─────────────────────────
/** The explanation on the side, arriving as it is read; the code and what the tools said beside it. */
function feature(id, { kicker, title, steps }) {
  const el = $('#' + id);
  el.innerHTML = `<div class="split"><div class="col side-text"><div class="kicker fx">${kicker}</div><h2 class="fx small"></h2><div class="say"></div></div>
    <div class="col"><div class="pane code-pane fx"><div class="bar"><span class="dots"><i></i><i></i><i></i></span><span class="title"></span></div><div class="body"></div></div><div class="outs"></div></div></div>`;
  let shown = null;
  B[id] = async (k, { fast, alive }) => {
    const s = steps[k];
    $('h2', el).textContent = s.title || title;
    const sy = $('.say', el);
    sy.innerHTML = '<p class="lede"></p>';
    say($('p', sy), s.say, fast);
    $('.code-pane .title', el).textContent = s.file;
    let typed = Promise.resolve();
    if (shown !== s.code || fast) { shown = s.code; typed = typeCode($('.code-pane .body', el), s.code, alive, { fast, ms: 20, hot: s.hot }); }
    else $$('.code-pane .body .ln', el).forEach(l => l.classList.toggle('hot', !!s.hot && s.hot.test(l.textContent)));
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

feature('contracts', {
  kicker: 'Types as contracts', title: 'Constraints on types.',
  steps: [
    { file: 'contracts', code: V.contracts.types, hot: /where/,
      say: 'Contract programming starts with the types. A type can carry a constraint: <code>Cents</code> is an <code>Int</code> that is never negative, and a <code>Percent</code> stays between 0 and 100. The condition is checked wherever a value of that type is made.' },
    { title: 'Preconditions and postconditions.', file: 'contracts', code: V.contracts.discount, hot: /requires|ensures/,
      say: 'A function has a contract too. <code>requires</code> is its precondition: what every caller must guarantee, here a price of at least 1.00. <code>ensures</code> is its postcondition: what holds for its result, called <code>it</code>, here that the discounted price is never more than the original.' },
    { title: 'Then the solver checks both sides.', file: 'contracts', code: V.contracts.callers, hot: /discount\(/, where: '~/contracts', outs: [V.contracts.verify],
      say: '<code>strata verify</code> passes the contracts to an SMT solver. It proves that <code>discount</code> meets its postcondition for every price and percentage, and checks every call against the precondition. <code>checkout</code> only calls it from 1.00 up, so it is proven. <code>sale</code> calls it with any price, and the solver gives the price that breaks it: 0.' },
  ],
});
feature('loops', {
  kicker: 'Loops and termination', title: 'Functions, and whether they end.',
  steps: [
    { file: 'examples/math.strata', code: V.loops.ackermann.code, hot: /ackermann\(/, where: '~/math', outs: [V.loops.ackermann.verify],
      say: '<code>verify</code> also checks that functions finish. For a recursive function it looks for a measure that goes down on every call and is never negative. For Ackermann\'s function, that is the pair <code>(m, n)</code>, compared in order.' },
    { title: 'Loops, and invariants found for you.', file: 'examples/math.strata', code: V.loops.isqrt.code, hot: /^\s*(while|ensures)/, where: '~/math', outs: [V.loops.isqrt.verify],
      say: 'A <code>while</code> loop ends when a bound goes down on every pass, and here it does. The postcondition needs a fact that holds on every pass, <code>r * r &lt;= n</code>: a loop invariant. For now it is fuzzed, and the output says so. Next, the verifier will find such facts itself: it guesses them from the loop\'s condition, the contract and the body, and keeps those that every pass preserves. What is left is proven. A written <code>invariant</code> clause is only planned for when that isn\'t enough.' },
    { title: 'Most of what a loop keeps is a type.', file: 'examples/math.strata', code: V.loops.collatz.code, hot: /mut|while/, where: '~/math', outs: [V.loops.collatz.verify],
      say: 'A variable\'s type is checked at every assignment and holds on every pass, so <code>x</code> stays positive through the loop without an invariant. Whether the Collatz sequence always reaches 1 is an open problem: the verifier proves every assignment, and reports that the loop may not end, with the bounds it tried.' },
    { title: 'Types can say what a list keeps.', file: 'records', code: V.loops.sorted.code, hot: /^type|best\[/, where: '~/records', outs: [V.loops.sorted.verify],
      say: 'The prover knows each element of a list by its position. So a type can say that a list is sorted, and <code>verify</code> proves that <code>record</code> keeps it sorted. <code>record_wrong</code> compares with the first record instead of the last, and the fuzzer gives the readings that break it.' },
  ],
});
feature('shares', {
  kicker: 'Proving over testing', title: 'Tests check the cases you thought of.',
  steps: [
    { file: 'shares', code: V.shares.code + '\n\n' + V.shares.tests, hot: /^test/, where: '~/shares', outs: [V.shares.test],
      say: 'A function that splits a bill between people, with three careful tests: an even split, one person, and a check that the shares add up. All three pass.' },
    { title: 'One line says what the tests meant.', file: 'shares', code: V.shares.contract, hot: /ensures/, where: '~/shares', outs: [V.shares.verify],
      say: 'The tests were all trying to say one thing: the shares add up to the total. Written as a contract, it covers every bill. <code>verify</code> finds one the tests never tried: 58 cents between 13 people, where 6 cents disappear. The tests only used totals that divide evenly.' },
    { title: 'Fuzzing: many tests at once.', file: 'shares', code: V.shares.fixed, hot: /extra/, where: '~/shares', outs: [V.shares.verify_fixed],
      say: 'Fixed: the remainder goes to the first shares. The solver can\'t prove a sum over a list yet, and says so. Where it can\'t, the fuzzer runs the function on 200 random inputs against the contract, different ones every run.' },
  ],
});
feature('points', {
  kicker: 'Proving over testing', title: 'Four tests, or one line.',
  steps: [
    { file: 'points', code: V.points.tests, hot: /^test/, where: '~/points', outs: [V.points.test],
      say: 'To pin down a range with tests, you test each edge: under a euro, one point per euro, the cap, never negative. Each test checks one value.' },
    { file: 'points', code: V.points.one, hot: /ensures/, where: '~/points', outs: [V.points.verify],
      say: 'One <code>ensures</code> says the same for every order, and the solver proves it. The four tests can go.' },
  ],
});

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

feature('mocks', {
  kicker: 'Proving over testing', title: 'Some tests still need mocks.',
  steps: [
    { file: 'mocks', code: F.mocks.src, hot: /^mock|^impl Files for MemDisk/, where: '~/mocks', outs: [F.mocks.test_out],
      say: 'Code that reads files or talks to a database still needs tests, and those need mocks. In Strata a mock is declared like a type, with the keyword <code>mock</code>, and implemented like any other type. It can leave methods out: <code>Files</code> has seven, and this one writes two.' },
    { file: 'mocks', code: F.mocks.src, hot: /^mock|^impl Files for MemDisk/, where: '~/mocks', outs: [F.mocks.verify_out],
      say: 'The methods it leaves out are generated, and answer within each method\'s types and contract. <code>verify</code> doesn\'t try to prove a mock: the tests that use it are what check it.' },
    { file: 'mocks', code: F.mocks.src, where: '~/mocks', outs: [F.mocks.refused],
      say: 'A mock can\'t end up in a real run: using one outside a test is refused, and the codebase stays as it was.' },
  ],
});
feature('capabilities', {
  kicker: 'Side effects', title: 'Capabilities.',
  steps: [
    { title: 'Side effects stay at the edges.', file: 'examples/wc', code: F.capabilities.main + '\n\n' + F.capabilities.wc, hot: /^fn main/,
      say: 'Side effects reach the outside world: the terminal, the disk, the network, C code. Nothing there can be proven, so their boundaries have to be clear and kept apart from the rest. Then the rest can be proven, and it is plain where verification stops.' },
    { title: 'Capabilities.', file: 'examples/wc', code: F.capabilities.main + '\n\n' + F.capabilities.wc, hot: /Console|Disk|Files/,
      say: 'Code reaches the outside world, the terminal, the disk or the network, only through capabilities. Only the runtime creates one, and only for <code>main</code>, which passes them on as traits like <code>Console</code> and <code>Files</code>. So the signatures show which code can have side effects.' },
    { title: 'The verifier knows where it stops.', file: 'examples/wc', code: F.capabilities.main + '\n\n' + F.capabilities.wc, hot: /^fn main|^fn wc/, where: '~/wc', outs: [F.capabilities.verify_out],
      say: 'The verifier can\'t reason about a disk, so it assumes the code that holds one, <code>main</code> here, and proves the rest: <code>wc</code> is checked against what <code>Console</code> and <code>Files</code> promise.' },
    { title: 'C code, the same way.', file: 'examples/fuzzy · module text', code: F.native.externs, hot: /^extern c/, where: '~/fuzzy', outs: [F.native.fuzzy_verify],
      say: '<code>extern c</code> declares a C function with a signature and contracts, checked at run time in both directions. The verifier assumes it, and only the module that declares it may call it, so the assumption stays in one place.' },
  ],
});

// ───────────────────────── what a change reaches ─────────────────────────
{
  const ss = D.session, g0 = ss[0].graph, graph = new Graph($('#build-graph'));
  const every = [...new Map(ss.flatMap(s => s.graph).map(d => [d.name, d])).values()];
  const fill = S('It fills the hole'), fix = S('It fixes it, and proves it');
  const iface = n => { const t = g0.find(d => d.name === n).text.split('\n').filter(l => !l.startsWith('///')); const i = t.findIndex(l => /\{\s*$/.test(l)); return t.slice(0, i + 1).join('\n').replace(/\s*\{\s*$/, ''); };
  const proof = fix.out.split('$ strata verify refund\n')[1];
  const STEPS = [
    { h: 'Signatures and contracts.', say: 'How <code>total</code> is written doesn\'t matter to <code>refund</code>: only that it does what its signature and contract say. So the solver proves <code>refund</code> from those alone. That is why code has functions, types and abstractions at all, and here it holds more than ever: an agent changing <code>refund</code>, or a person reviewing it, can leave the bodies around it unread.',
      term: ['strata verify refund', { cmd: 'strata verify refund', out: proof }], card: ['what the proof relies on', [iface('total'), g0.find(d => d.name === 'Cents').text.split('\n').filter(l => !l.startsWith('///')).join('\n')].join('\n\n')],
      graph: fast => graph.set(fix.graph, { layout: every, focus: 'refund', fast }) },
    { h: 'A change inside a body stays there.', say: 'Strata works out what each change reaches. The fix to <code>refund</code> changed its body, not its signature or contract, so it reaches nothing else: nothing that uses it has to be checked again, and the edit says so. A change to a contract would reach every user, and only them.',
      term: ['~/shop', { cmd: fix.cmd, out: fix.out.split('\n\n$ strata verify')[0] }], graph: fast => graph.set(fix.graph, { layout: every, changed: new Set(['refund']), fast }) },
    { h: 'Unfinished code still checks.', say: 'A definition can be left as a hole, <code>?refund_policy</code> here, and everything around it still type-checks and verifies. <code>strata holes</code> says what is missing: the type to produce, what is in scope, and the contract to meet.',
      term: ['~/shop', { cmd: 'strata holes', out: D.orders.holes }], card: ['refund, for now', g0.find(d => d.name === 'refund').text], graph: fast => graph.set(g0, { layout: every, focus: 'refund', fast }) },
    { h: 'Fill one hole, and move on.', say: 'The agent fills that one hole. The edit is checked against everything it touches, and stored. Code gets built a piece at a time, and it checks at every step.',
      term: ['~/shop', { ...fill, input: null }], card: ['piped into strata edit', fill.input], graph: fast => graph.set(fill.graph, { layout: every, changed: new Set(['refund', 'test "refund keeps the handling fee"']), focus: 'refund', fast }) },
  ];
  B.build = async (k, { fast, alive }) => {
    const s = STEPS[k];
    $('#build-h').textContent = s.h;
    $('#build-say').innerHTML = '<p></p>'; say($('#build-say p'), s.say, fast);
    s.graph(fast);
    const card = $('#build-code');
    card.classList.toggle('on', !!s.card);
    if (s.card) { $('#build-code-title').textContent = s.card[0]; typeCode($('.body', card), s.card[1], alive, { fast, ms: 24 }); }
    $('#build-term-title').textContent = s.term[0];
    await typeTerm($('#build-term'), s.term[1], alive, { fast });
  };
}

// ───────────────────────── text → tokens → tree → graph ─────────────────────────
/** Strata's tokens, for the animations (comments and spaces are kept as such, to be dropped). */
function tokenize(src) {
  const re = /(\n)|([ \t]+)|(\/\/[^\n]*)|("(?:[^"\\]|\\.)*")|(\d+)|([A-Za-z_][A-Za-z0-9_]*)|(->|==|!=|>=|<=|&&|\|\||=>|:=)|([^\s])/g;
  const out = []; let m;
  while ((m = re.exec(src))) {
    const t = m[0];
    out.push({ t, kind: m[1] ? 'nl' : m[2] ? 'ws' : m[3] ? 'c' : m[4] ? 's' : m[5] ? 'nm' : m[6] ? (KW.has(t) ? 'k' : /^[A-Z]/.test(t) ? 't' : 'f') : 'p' });
  }
  return out;
}
/** A small parser for the definitions the slides animate: types, and functions with contracts and one expression. */
function parseDefs(src) {
  const toks = tokenize(src).filter(x => !['nl', 'ws', 'c'].includes(x.kind)).map(x => x.t);
  let i = 0;
  const peek = () => toks[i], next = () => toks[i++], eat = t => { if (toks[i] !== t) throw new Error(`expected ${t} at ${toks[i]}`); i++; };
  const N = (t, k, c = []) => ({ t, k, c });
  const type = () => { const n = next(); if (peek() === '[') { next(); const a = [type()]; while (peek() === ',') { next(); a.push(type()); } eat(']'); return N(n, 'type', a); } return N(n, 'type'); };
  const PREC = [['||'], ['&&'], ['==', '!=', '<', '<=', '>', '>='], ['+', '-'], ['*', '/', '%']];
  const args = () => { const a = []; while (peek() !== ')') { a.push(expr()); if (peek() === ',') next(); } eat(')'); return a; };
  const primary = () => { const t = next(); if (t === '(') { const e = expr(); eat(')'); return e; } return N(t, /^\d/.test(t) ? 'num' : t === 'it' ? 'kw' : 'id'); };
  const postfix = () => {
    let e = primary();
    for (;;) {
      if (peek() === '.') { next(); const f = next(); if (peek() === '(') { next(); e = N(`.${f}()`, 'call', [e, ...args()]); } else e = N(`.${f}`, 'field', [e]); }
      else if (peek() === '(') { next(); e = N(`${e.t}()`, 'call', args()); }
      else return e;
    }
  };
  const expr = (lv = 0) => { if (lv === PREC.length) return postfix(); let l = expr(lv + 1); while (PREC[lv].includes(peek())) { const op = next(); l = N(op, 'op', [l, expr(lv + 1)]); } return l; };
  const fn = () => {
    eat('fn'); const name = next(); eat('(');
    const kids = [];
    while (peek() !== ')') { const p = next(); eat(':'); kids.push(N(p, 'param', [type()])); if (peek() === ',') next(); }
    eat(')'); eat('->'); kids.push(N('→', 'result', [type()]));
    while (peek() === 'requires' || peek() === 'ensures') { const k = next(); kids.push(N(k, 'contract', [expr()])); }
    eat('{'); kids.push(N('body', 'body', [expr()])); eat('}');
    return N(`fn ${name}`, 'def', kids);
  };
  const typeDecl = () => {
    eat('type'); const name = next(); eat('=');
    if (peek() === '{') { next(); const fs = []; while (peek() !== '}') { const f = next(); eat(':'); fs.push(N(f, 'param', [type()])); if (peek() === ',') next(); } eat('}'); return N(`type ${name}`, 'def', fs); }
    const kids = [type()]; if (peek() === 'where') { next(); kids.push(N('where', 'contract', [expr()])); }
    return N(`type ${name}`, 'def', kids);
  };
  const defs = [];
  while (i < toks.length) defs.push(peek() === 'fn' ? fn() : typeDecl());
  return defs;
}
/** Source as tokens, laid out as the text was: `chips` turns them into separate tokens, without the spaces. */
function showTokens(el, src) {
  el.innerHTML = tokenize(src).map((x, i) => x.kind === 'nl' ? '<br>' : `<span class="tok ${x.kind}" style="--i:${i}">${esc(x.t)}</span>`).join('');
}
/** A syntax tree drawn top-down, growing from the root. */
function drawTree(el, roots, fast) {
  el.innerHTML = '<svg></svg>';
  const all = []; let leaf = 0;
  const lay = (n, d) => { const kids = n.c.map(c => lay(c, d + 1)); const x = kids.length ? (kids[0].x + kids.at(-1).x) / 2 : leaf++; const me = { n, d, x, kids }; all.push(me); return me; };
  for (const r of roots) { lay(r, 0); leaf += .5; }
  const depth = Math.max(...all.map(m => m.d)) + 1, W = el.clientWidth, H = el.clientHeight;
  const X = x => 50 + (x + .5) / (leaf + .5) * (W - 100), Y = d => 50 + d * Math.min(90, (H - 100) / Math.max(1, depth - 1));
  const delay = m => fast ? 0 : m.d * 160 + m.x * 25;
  el.firstChild.setAttribute('viewBox', `0 0 ${W} ${H}`);
  el.firstChild.innerHTML = all.flatMap(m => m.kids.map(c => `<path d="M${X(m.x)} ${Y(m.d) + 12} C${X(m.x)} ${(Y(m.d) + Y(c.d)) / 2}, ${X(c.x)} ${(Y(m.d) + Y(c.d)) / 2}, ${X(c.x)} ${Y(c.d) - 12}" style="animation-delay:${delay(c)}ms"/>`)).join('');
  el.insertAdjacentHTML('beforeend', all.map(m => `<div class="tnode k-${m.n.k}" style="left:${X(m.x)}px;top:${Y(m.d)}px;animation-delay:${delay(m)}ms">${esc(m.n.t)}</div>`).join(''));
}
{
  const g0 = D.session[0].graph, txt = n => g0.find(d => d.name === n).text;
  const names = ['Cents', 'Quantity', 'Line', 'line_total', 'total'];
  const src = names.slice(2).map(txt).join('\n\n');
  const decls = g0.filter(d => names.includes(d.name)).map(d => ({ ...d, verdict: null, holes: 0 }));
  const stage = $('#astage'), graph = new Graph($('#ast-graph'), { legend: false });
  const show = layer => stage.dataset.show = layer;
  const STEPS = [
    { h: 'Code is written as text.', say: 'Here is a small part of an order system, as anyone would write it: a type for one line of an order, and two functions over it.' },
    { h: 'A compiler first reads it as tokens.', say: 'Names, keywords and symbols, one after the other. The spaces, the line breaks and the layout are dropped: they were only there for whoever reads the text.' },
    { h: 'Then it builds a tree.', say: 'Each definition becomes a syntax tree. <code>line_total</code> is a function with a parameter, a result type, a contract, and a body that multiplies two fields. The tree is what the code means; the text was one way of writing it down.' },
    { h: 'The trees link up into a graph.', say: 'Names point to other definitions: <code>line_total</code> uses <code>Line</code> and <code>Cents</code>, and <code>total</code> uses <code>line_total</code>. Across a codebase, these links form a graph of types and functions.' },
    { h: 'Running the program walks that graph.', say: 'A call follows the edges. <code>total</code> calls <code>line_total</code> once per line, which reads the line\'s fields, and the results come back up.' },
  ];
  B.ast = async (k, { fast, alive }) => {
    const s = STEPS[k];
    $('#ast-h').textContent = s.h;
    $('#ast-say').innerHTML = '<p></p>'; say($('#ast-say p'), s.say, fast);
    if (k === 4) { $('#ast-title').textContent = '$ strata run total … --trace'; $('#ast-code').innerHTML = ''; typeTerm($('#ast-code'), { cmd: 'strata run total \'[[{"sku":"tea","qty":2,"unit_price":450},{"sku":"cup","qty":1,"unit_price":1200}]]\' --trace', out: D.orders.trace }, alive, { fast }); }
    else { $('#ast-title').textContent = 'orders.strata'; $('#ast-code').innerHTML = hl(src); }
    if (k <= 1) {
      show('tokens');
      const el = $('#ast-tokens');
      if (k === 0 || !el.children.length) showTokens(el, src);
      el.classList.toggle('chips', k === 1);
      return;
    }
    if (k === 2) { show('tree'); await sleep(fast ? 0 : 50); drawTree($('#ast-tree'), parseDefs(txt('line_total')), fast); return; }
    show('graph');
    if (k === 3) {
      if (fast) { graph.set(decls, { statuses: false, fast }); return; }
      graph.set([], { layout: decls });
      for (const n of ['Cents', 'Quantity', 'Line', 'line_total', 'total']) { await sleep(450); if (!alive()) return; graph.set(decls.filter(d => names.slice(0, names.indexOf(n) + 1).includes(d.name)), { layout: decls, statuses: false, changed: new Set([n]) }); }
      return;
    }
    graph.set(decls, { statuses: false, fast: true });
    const walk = ['total', 'line_total', 'Line', 'line_total', 'total', 'line_total', 'Line', 'line_total', 'total'];
    while (alive()) {
      for (let j = 0; j < walk.length && alive(); j++) {
        graph.set(decls, { statuses: false, focus: walk[j], lit: new Set(j && walk[j - 1] !== walk[j] ? [walk[j]] : []), fast: true });
        await sleep(650);
      }
      await sleep(900);
    }
  };
}

// ───────────────────────── Strata stores the graph ─────────────────────────
{
  const g0 = D.session[0].graph, graph = new Graph($('#tree-graph'));
  const at = graph.layout(g0);
  const order = [...g0].sort((a, b) => at.get(a.name).x - at.get(b.name).x || at.get(a.name).y - at.get(b.name).y);
  B.tree = async (k, { fast, alive }) => {
    const code = $('#tree-code');
    $('#tree-say').innerHTML = '<p></p>';
    say($('#tree-say p'), 'Strata keeps each definition as its tree, with a content hash, together with the links between them. This is the order system: as each definition is written, it takes its place in the graph next to what it uses. Text is produced from the trees when someone wants to read it.', fast);
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
  };
}

// ───────────────────────── structural changes ─────────────────────────
{
  const R = D.rename, renameV = S('A rename is one change'), fill = S('It fills them, and closes the change'), fix = S('It fixes it, and proves it');
  const C = D.contract;
  const big = diffCount(R.diff), bigLines = R.diff.split('\n').length;
  // each step: the line diffs and the structural changes it shows, side by side
  const ch = (snap, step) => ({ linediff: snap === R ? R.diff : snap.linediff, change: snap.changes[step], title: snap.changes.all.steps.find(s => s.id === step).title });
  const cases = [
    { tab: 0, parts: [ch(R, R.step), ch(renameV, lastStep(renameV))], h: 'A rename should read as a rename.',
      say: `Two renames. <code>strata rename Task Job</code> renamed a type, and the twelve definitions that use it were updated; <code>strata rename Shipped Sent</code> renamed a variant, and every pattern and test that uses it. As text, that is ${big + diffCount(renameV.linediff)} changed lines. As changes, it is two lines.` },
    { tab: 1, parts: [{ ...ch(C, stepTitled(C, 'discount: members')), linediff: C.paramDiff }], h: 'A new parameter, and the calls that follow it.',
      say: `<code>discount</code> takes a new <code>member</code> flag, not used yet. The change is the new parameter, and its two call sites are listed as following it.` },
    { tab: 1, parts: [{ ...ch(fill, stepTitled(fill, 'coupons')), linediff: D.orders.couponsDiff }], h: 'And when the body changes too.',
      say: `<code>total</code> takes a coupon and uses it. The new signature and the lines of the body that changed are shown in place; the callers that had to follow are listed by name.` },
  ];
  $('#diff-tabs').innerHTML = ['Renames', 'A new parameter'].map((t, i) => `<button data-i="${i}">${t}</button>`).join('');
  $('#diff-tabs').addEventListener('click', e => { const b = e.target.closest('button'); if (b) { const k = cases.findIndex(c => c.tab === +b.dataset.i) + 1; go(cur, k, k < sub ? -1 : 1); } });
  const count = async (el, n, unit, alive, ms = 260) => { for (let i = 0; i <= n; i++) { if (!alive()) return; el.textContent = `${i} ${unit}${i === 1 ? '' : 's'}`; await sleep(Math.max(12, ms / n)); } };
  const flip = el => { el.classList.remove('flip'); void el.offsetWidth; el.classList.add('flip'); };
  $('#big-body').innerHTML = diffHtml(R.diff);
  B.diffs = async (k, { fast, alive }) => {
    $('#diffs').classList.toggle('big', k === 0);
    if (k === 0) {
      $('#diff-h').textContent = 'Here is a diff. What happened?'; flip($('#diff-h'));
      say($('#diff-say'), `A change to <code>examples/tasks</code>, as <code>git diff</code> shows it: ${bigLines} lines of diff over four modules, ${big} of them changed. To review it, you have to read all of them.`, fast);
      const body = $('#big-body');
      body.scrollTop = 0;
      count($('#big-count'), big, 'line', alive, 1600).then(() => alive() && ($('#big-count').textContent = `${big} lines changed`));
      await sleep(fast ? 200 : 700);
      while (alive()) {
        body.scrollTop += 9;
        if (body.scrollTop + body.clientHeight >= body.scrollHeight - 1) { await sleep(900); if (!alive()) return; body.scrollTop = 0; await sleep(400); }
        await new Promise(requestAnimationFrame);
      }
      return;
    }
    const c = cases[k - 1];
    $('#diff-h').textContent = c.h; flip($('#diff-h'));
    say($('#diff-say'), c.say, fast);
    $$('#diff-tabs button').forEach((b, j) => b.classList.toggle('on', j === c.tab));
    const sts = c.parts.map(p => structural(p.change));
    const n = c.parts.reduce((t, p) => t + diffCount(p.linediff), 0), st = { count: sts.reduce((t, s) => t + s.count, 0) };
    const sep = (p, j) => c.parts.length > 1 ? `<div class="part-title">${j + 1}. ${esc(p.title)}</div>` : '';
    $('#ld-body').innerHTML = c.parts.map((p, j) => sep(p, j) + diffHtml(p.linediff)).join('<br>');
    $('#sd-body').innerHTML = sts.map((s, j) => sep(c.parts[j], j) + s.html).join('');
    for (const id of ['#ld-body', '#sd-body']) flip($(id));
    $('#ld-count').textContent = `${n} line${n === 1 ? '' : 's'} changed`;
    $('#sd-count').textContent = `${st.count} change${st.count > 1 ? 's' : ''}`;
    $('#sd-title').textContent = `strata diff · ${c.parts.map(p => p.title).join(', ')}`;
    if (fast) { $('#ld-n').textContent = `${n} lines`; $('#sd-n').textContent = `${st.count} change${st.count > 1 ? 's' : ''}`; }
    else { count($('#ld-n'), n, 'line', alive); count($('#sd-n'), st.count, 'change', alive); }
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
    'Every change is checked before it is stored, so no agent can leave the codebase broken for the others. Several agents can work at the same time, each on its own branch and its own task. Here, agent A renames <code>line_total</code> on <code>main</code>.',
    'Meanwhile agent B rewrites the body of <code>line_total</code> on its branch. As text, the two changes touch the same lines, which would be a merge conflict.',
    'Strata merges definitions, not lines. It applies A\'s rename to B\'s code and checks the result before committing it. Agents working on different definitions never conflict, even in the same file; here they didn\'t even conflict on the same function.',
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


// ───────────────────────── changes follow the graph ─────────────────────────
{
  const C = D.contract, graph = new Graph($('#travel-graph'));
  const every = [...new Map(C.steps.flatMap(s => s.graph).map(d => [d.name, d])).values()];
  const tighten = C.steps.find(s => s.title === 'Tighten discount’s contract');
  B.travel = async (k, { fast, alive }) => {
    const card = $('#travel-code'), stamp = $('#travel-stamp');
    stamp.className = 'stamp';
    if (k === 0) {
      sayBlock($('#travel-say'), 'What happens when contracts change?',
        '<code>discount</code> is about to require a price of at least 1.00. Every caller that passes less now breaks, wherever it is. In a codebase of text files, finding them means searching and hoping. In Strata, the edit follows the edges of the graph.', fast, '<div class="who you"><i></i>the problem</div>');
      graph.set(C.steps[0].graph, { layout: every, focus: 'discount', fast });
      card.classList.remove('on');
      $('#travel-term-title').textContent = 'git diff';
      $('#travel-term').innerHTML = diffHtml(tighten.linediff);
      return;
    }
    const s = C.steps[k - 1], before = C.steps[k - 2]?.graph;
    sayBlock($('#travel-say'), esc(s.title), md(s.say), fast, `<div class="who"><i></i>step ${k} of ${C.steps.length}</div>`);
    const cex = {};
    for (const d of s.graph) if (d.verdict === 'counterexample') cex[d.name] = `${esc(d.name)}(${(d.args || []).map(a => esc(JSON.stringify(a))).join(', ')}): ${esc((d.message || '').replace(/\s*\(stack:.*$/, ''))}`;
    const changed = before ? changedBetween(before, s.graph) : new Set(['checkout']);
    graph.set(s.graph, { layout: every, changed, lit: before ? ifaceChanged(before, s.graph) : new Set(), cex, fast });
    $('#travel-term-title').textContent = '~/shop';
    card.classList.toggle('on', !!s.input);
    const typing = s.input ? typeCode($('.body', card), s.input, alive, { fast, ms: 18 }) : null;
    await typeTerm($('#travel-term'), { ...s, input: null }, alive, { fast });
    await typing;
    if (s.input && alive()) { await sleep(fast ? 0 : 1600); if (alive()) card.classList.remove('on'); }
  };
}

// ───────────────────────── only what it needs ─────────────────────────
{
  const g0 = D.session[0].graph, L = D.orders.levels, graph = new Graph($('#views-graph'));
  const all = g0.map(d => d.text).join('\n\n');
  const STEPS = [
    { h: 'Code is large.', say: 'Even this small codebase has twenty definitions; a real one has thousands, across hundreds of files. An agent working on orders doesn\'t need all of it. It needs <code>Order</code>, what it uses, and what uses it, and to change things around it.',
      title: 'orders.strata', text: all },
    { h: 'So it asks for a slice.', say: '<code>strata view slice:Order</code> renders exactly that part of the graph, from the trees. The first level is a map: one line per definition, with what it uses and its status.', title: '$ strata view slice:Order --level 0', text: L[0], focus: 'Order' },
    { h: 'At the detail it needs.', say: 'The second level shows interfaces: enough to use code without reading how it works. With contracts, that is usually all an agent needs to read.', title: '$ strata view slice:Order --level 1', text: L[1], focus: 'Order' },
    { h: 'At the detail it needs.', say: 'The third level is the source. All three are rendered from the same trees, in one canonical format, so no file layout or formatting gets in the way.', title: '$ strata view slice:Order --level 2', text: L[2], focus: 'Order' },
  ];
  B.views = (k, { fast, alive }) => {
    const s = STEPS[k];
    $('#views-h').textContent = s.h;
    $('#views-say').innerHTML = '<p></p>'; say($('#views-say p'), s.say, fast);
    $('#views-title').textContent = s.title;
    $('#views-count').textContent = `${s.text.trimEnd().split('\n').length} lines`;
    graph.set(g0, { statuses: k > 0, focus: s.focus || null, fast });
    return typeCode($('#views-code'), s.text.trimEnd(), alive, { fast, ms: k ? 10 : 3 });
  };
}

// ───────────────────────── editing with text ─────────────────────────
{
  const E = V.edit, g0 = D.session[0].graph, graph = new Graph($('#edit-graph'));
  const stage = $('#estage'), show = layer => stage.dataset.show = layer;
  const STEPS = [
    { h: 'Agents still write text.', say: 'An agent doesn\'t need to know about trees. It writes the definition it wants to change as text, in whatever layout, and sends it to <code>strata edit</code>. Only that definition, not the file around it.' },
    { h: 'The text is parsed into a tree.', say: 'Strata reads it the way a compiler does and keeps only the tree. The spacing it was typed with is gone.' },
    { h: 'The tree replaces the old one, if it checks.', say: 'The new tree takes the place of <code>line_total</code> in the graph, and the code around it is checked with it before anything is stored. Its contract didn\'t change, so nothing else has to be checked again.' },
    { h: 'And it is rendered back as text.', say: 'Whoever reads it next gets the canonical format, the same for every agent and every person. There is no formatter to run, and no style to argue about.' },
    { h: 'An edit that doesn\'t check never gets in.', say: 'This one multiplies a price by a name. It is refused, and the codebase stays as it was.' },
  ];
  B.edit = async (k, { fast, alive }) => {
    const s = STEPS[k], T = $('#edit-term');
    $('#edit-h').textContent = s.h;
    $('#edit-say').innerHTML = '<p></p>'; say($('#edit-say p'), s.say, fast);
    $('#edit-term-title').textContent = '~/shop';
    stage.classList.remove('shake');
    if (k === 4) {
      show('graph');
      graph.set(g0, { focus: 'line_total', fast: true });
      await typeTerm(T, { ...E.refused, input: E.bad }, alive, { fast });
      if (alive()) { $('#edit-graph').classList.remove('shake'); void stage.offsetWidth; graph.shake(); }
      return;
    }
    if (k <= 1) {
      T.innerHTML = cmdLine(E.stored.cmd) + piped(E.sloppy);
      const el = $('#edit-tokens');
      if (k === 0) { show('tokens'); showTokens(el, E.sloppy); el.classList.remove('chips'); return; }
      show('tokens'); showTokens(el, E.sloppy); void el.offsetWidth; el.classList.add('chips');
      await sleep(fast ? 0 : 1400); if (!alive()) return;
      show('tree'); await sleep(50);
      drawTree($('#edit-tree'), parseDefs(E.sloppy), fast);
      return;
    }
    if (k === 2) {
      show('graph');
      graph.set(g0, { focus: 'line_total', changed: new Set(['line_total']), fast });
      return typeTerm(T, { ...E.stored, input: E.sloppy }, alive, { fast });
    }
    show('rendered');
    T.innerHTML = cmdLine(E.stored.cmd) + piped(E.sloppy) + term(E.stored.out);
    T.insertAdjacentHTML('beforeend', cmdLine('strata view line_total --level 2'));
    await typeCode($('#edit-rendered'), E.rendered, alive, { fast, ms: 70 });
  };
}

// ───────────────────────── more of the language ─────────────────────────
{
  const L = V.lang, K = 'More of the language';
  feature('adt', {
    kicker: K, title: 'Algebraic data types.',
    steps: [
      { file: 'shapes', code: L.adt.src, hot: /^type|\| |where it/, where: '~/shapes', outs: [L.adt.run],
        say: 'Types are built from records and variants: a <code>Shape</code> is a circle, a rectangle or a point, each with its own fields. The same building blocks give <code>Option</code> and <code>Result</code>, so a missing value or an error is part of the type. With constraints on top, types can be as precise as the domain needs. <code>match</code> takes values apart, and an arm can test a condition with <code>where</code>.' },
      { title: 'Every case is covered.', file: 'shapes', code: L.adt.src, hot: /match s|Point =>/, where: '~/shapes', outs: [L.adt.refused],
        say: 'A <code>match</code> has to cover every case. Leave <code>Point</code> out, and the edit is refused before anything is stored.' },
    ],
  });
  feature('traits', {
    kicker: K, title: 'Traits.',
    steps: [
      { file: 'priced', code: L.traits.src, hot: /^trait|^impl|Priced\)/, where: '~/priced', outs: [L.traits.run],
        say: 'Traits are inspired by Rust\'s. A type implements a trait, and a trait can give its methods default bodies. They are how Strata does polymorphism, by composition instead of inheritance: <code>receipt</code> takes anything <code>Priced</code>, and one list can hold different types that share a trait.' },
    ],
  });
  feature('values', {
    kicker: K, title: 'Values, and visible mutation.',
    steps: [
      { file: 'cart', code: L.values.src, hot: /mut/, where: '~/cart', outs: [L.values.run],
        say: 'Values are never shared: changing <code>b</code> leaves <code>a</code> as it was. Memory is managed by reference counting, so this is cheap, and a value is only copied when it is changed while something else still holds it. Everything is immutable unless it is declared <code>mut</code>, and a function that changes its argument says <code>mut</code> in its signature and at every call.' },
      { title: 'No state changes unseen.', file: 'cart', code: L.values.src, hot: /add\(mut b/, where: '~/cart', outs: [L.values.refused],
        say: 'Leave <code>mut</code> out at the call, and the edit is refused. Reading any line of code, you can see what it may change.' },
    ],
  });
  feature('sugar', {
    kicker: K, title: 'The everyday syntax.',
    steps: [
      { file: 'lines', code: V.sugar.src, hot: /for l in|f"|->/, where: '~/lines', outs: [V.sugar.run],
        say: 'List and dictionary comprehensions with filters, string interpolation with format specs, lambdas, and method syntax on any function. The everyday syntax stays close to Rust and Python.' },
    ],
  });
  feature('comptime', {
    kicker: K, title: 'Code that runs while compiling.',
    steps: [
      { file: 'tables', code: L.comptime.src, hot: /comptime/, where: '~/tables', outs: [L.comptime.run],
        say: '<code>comptime e</code> is evaluated by the compiler, and the program keeps only the value: here the primes below 60, and a table of squares. The evaluation is deterministic, and its results are cached between builds.' },
      { title: 'Derived code is built on it.', file: 'examples/hello', code: F.derive.options, hot: /^derive/, where: '~/hello', outs: [F.derive.help, F.derive.bad],
        say: '<code>derive CommandLine for Options</code> is Strata code that runs while compiling. It reads the type\'s fields, doc comments, defaults and constraints, and generates an ordinary <code>impl</code>: a parser, with its help text, that refuses values outside the constraints.' },
      { title: 'The same for JSON.', file: 'signup', code: F.derive.json, hot: /^derive|where/, where: '~/signup', outs: [F.derive.json_run],
        say: '<code>derive Json</code> generates the encoder and the decoder: a signup is written to JSON and read back. Reading checks every constraint, and an error names the field and the condition it broke.' },
    ],
  });
  feature('tasks', {
    kicker: K, title: 'Concurrency, without coloring.',
    steps: [
      { file: 'concurrency: designed, not built yet', code: L.tasks.spawn, hot: /scope \{|spawn |await /,
        say: 'Decided, not built yet: there is one kind of function, and any function may block, so there is no <code>async</code> marking callers. <code>spawn</code> starts a task and <code>await</code> takes its result, for one task, a tuple or a list. A task belongs to its <code>scope</code>, which ends only once every task started in it has. Tasks share nothing that can change, so to <code>verify</code>, <code>await (spawn f(), spawn g())</code> is just <code>(f(), g())</code>.' },
      { title: 'Channels, and waiting on several things.', file: 'concurrency: designed, not built yet', code: L.tasks.channels, hot: /channel\[|\.send|\.close|select \{|\) as |download as/,
        say: 'Tasks talk through channels. Each one has a fixed size, so a fast sender waits for its receivers, and every sender has to be closed, which the checker sees. <code>select</code> waits on several operations and completes exactly one: the others did nothing, so a message that lost is still in the channel.' },
    ],
  });
}

// ───────────────────────── under the hood ─────────────────────────
{
  const facts = [
    ['LLVM', 'One code generator. <code>run</code>, <code>test</code> and <code>verify</code> compile on the JIT, <code>strata build</code> makes executables.'],
    ['RC', 'Memory by reference counting: a value is freed when nothing holds it, and updated in place when only one thing does.'],
    ['std', 'Written in Strata and precompiled. Rust is used only for the native I/O layer.'],
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
