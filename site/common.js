// Shared by the presentation (app.js) and the docs pages (docs.html):
// highlighting for Strata code, strata's terminal output and line diffs.
'use strict';
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const sleep = ms => new Promise(r => setTimeout(r, ms));

const KW = new Set(('fn type trait impl for in if else match while loop return break continue mut let pub use test assert ' +
  'requires ensures where it self Self true false mock distinct derive derivable derived comptime or except invariant law as is').split(' '));
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
