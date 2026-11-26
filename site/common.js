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
