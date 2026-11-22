// Shared by the presentation (app.js) and the docs pages (docs.html):
// highlighting for Strata code, strata's terminal output and line diffs.
'use strict';
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const sleep = ms => new Promise(r => setTimeout(r, ms));

const KW = new Set(('fn type trait impl for in if else match while loop return break continue mut let pub use test assert ' +
  'requires ensures where it self Self true false mock distinct derive derivable derived comptime or except invariant law as is scope spawn await select').split(' '));
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

