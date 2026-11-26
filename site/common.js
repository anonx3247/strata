// Shared by the tour (app.js) and the landing page (landing.js):
// highlighting for Strata code, strata's terminal output and line diffs.
'use strict';
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const sleep = ms => new Promise(r => setTimeout(r, ms));

const KW = new Set(('fn type trait impl for in if else match while loop return break continue mut let pub use test assert ' +
  'requires ensures where it self Self true false mock distinct derive derivable derived comptime or except invariant law as is scope spawn await select race supervised from capability extern terminates noalloc lemma by calc old model decreases shared').split(' '));
/** Strata source, highlighted. */
function hl(code) {
  const re = /(\/\/[^\n]*)|(f?"(?:[^"\\]|\\.)*")|(\?[a-z_][a-z0-9_]*)|(\b\d[\d_]*(?:\.\d+)?\b)|([A-Za-z_][A-Za-z0-9_]*)(?=\s*\()|([A-Za-z_][A-Za-z0-9_]*)|([\s\S])/g;
  let out = '', m;
  while ((m = re.exec(code))) {
    if (m[1]) out += `<span class="${m[1].startsWith('///') ? 'c doc' : 'c'}">${esc(m[1])}</span>`;
    else if (m[2]) out += `<span class="s">${esc(m[2])}</span>`;
    else if (m[3]) out += `<span class="h">${esc(m[3])}</span>`;
    else if (m[4]) out += `<span class="nm">${m[4]}</span>`;
    else if (m[5]) out += KW.has(m[5]) ? `<span class="k">${m[5]}</span>` : /^[A-Z]/.test(m[5]) ? `<span class="t">${m[5]}</span>` : `<span class="f">${m[5]}</span>`;
    else if (m[6]) out += KW.has(m[6]) ? `<span class="k">${m[6]}</span>` : /^[A-Z]/.test(m[6]) ? `<span class="t">${m[6]}</span>` : esc(m[6]);
    else out += esc(m[7]);
  }
  return out;
}
/** A line of strata's terminal output, coloured by what it means. */
function termLine(l) {
  const e = esc(l);
  if (/^\$ /.test(l)) {
    const [, cmd, com] = l.slice(2).match(/^(.*?)(\s{2,}#.*)?$/);
    return `<span class="prompt">$</span> <span class="cmd">${esc(cmd)}</span>${com ? `<span class="c">${esc(com)}</span>` : ''}`;
  }
  if (/^\s*✓/.test(l)) return `<span class="o-ok">${e}</span>`;
  if (/^\s*✗|^error|^rejected|^fault/.test(l)) return `<span class="o-bad">${e}</span>`;
  if (/^\s*[~○]\s/.test(l)) return `<span class="o-warn">${e}</span>`;
  if (/^\s+input:/.test(l)) return `<span class="o-in">${e}</span>`;
  if (/^committed to git|^Auto-merging|^Automatic merge/.test(l)) return `<span class="o-dim">${e}</span>`;
  if (/^\s*\?[a-z_]/.test(l)) return e.replace(/(\?[a-z_0-9]+)/, '<span class="h">$1</span>');
  if (/^strata: .*applied/.test(l)) return `<span class="o-acc">${e}</span>`;
  if (/^\s*re-check dependents/.test(l)) return `<span class="o-warn">${e}</span>`;
  if (/^(fn|type|test|trait|impl)\s+\S/.test(l)) return e.replace(/(#[0-9a-f]{8,}…?)/g, '<span class="o-dim">$1</span>').replace(/(added|changed)/, '<b>$1</b>').replace(/\((interface changed)\)/, '(<span class="o-warn">$1</span>)');
  return e.replace(/(#[0-9a-f]{8,}…?)/g, '<span class="o-dim">$1</span>');
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
/** A shell snippet: commands, with `# comments` dimmed. */
const shHtml = src => src.split('\n').map(l => { const m = l.match(/^(.*?)(\s*#.*)?$/); return esc(m[1]) + (m[2] ? `<span class="c">${esc(m[2])}</span>` : ''); }).join('\n');

// ───────────────────────── steppers: a bar of steps, arrows, and the ← → keys ─────────────────────────
// Anything that ← → can drive: steppers and tab rows. {root, prev(), next()}
const KEYS = [];
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
    KEYS.push(this);
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
// ← → drive whichever stepper or tab row takes up most of the screen; inside an
// embedded viewer, the one holding that viewer (the viewer passes the keys up)
function arrow(key, from) {
  const seen = s => { const r = s.root.getBoundingClientRect(); return Math.max(0, Math.min(r.bottom, innerHeight) - Math.max(r.top, 0)); };
  const holder = from && KEYS.find(k => k.root.contains(from));
  const best = holder ? [holder, Infinity] : KEYS.map(s => [s, seen(s)]).sort((a, b) => b[1] - a[1])[0];
  if (!best || best[1] < Math.min(innerHeight * 0.2, 160)) return false;
  key === 'ArrowLeft' ? best[0].prev() : best[0].next();
  return true;
}
addEventListener('keydown', e => {
  if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
  if (e.metaKey || e.ctrlKey || e.altKey || /INPUT|TEXTAREA|SELECT/.test(e.target.tagName)) return;
  if (arrow(e.key)) e.preventDefault();
});
addEventListener('message', e => {
  if (e.data?.type !== 'strata-key') return;
  const frame = [...document.querySelectorAll('iframe')].find(f => f.contentWindow === e.source);
  arrow(e.data.key, frame);
});
/** A row of tabs that ← → also move through. */
function arrowTabs(root, n, pick, cur) {
  KEYS.push({ root, prev: () => pick(Math.max(0, cur() - 1)), next: () => pick(Math.min(n - 1, cur() + 1)) });
}
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
