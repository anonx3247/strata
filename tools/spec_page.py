"""SPEC.md as one HTML page, for the spec's artifact: python3 tools/spec_page.py SPEC.md OUT.html"""

import html
import re
import sys

SECTIONS = [
    ("Lexical structure", "draft"),
    ("Syntax", "waits on lab trials 3, 4, 5"),
    ("The tree", "draft"),
    ("Binary format and hashing", "draft"),
    ("Names and modules", "to write"),
    ("Types", "to write"),
    ("Semantics", "waits on lab trials 3, 4, 5"),
    ("Verification", "waits on kernel round 2"),
    ("Concurrency", "to write"),
    ("Memory", "waits on the memory trial"),
    ("Runtime and extern c", "waits on #557"),
    ("Core library", "after section 11"),
]

KEYWORDS = set("""fn type trait impl test law lemma extern pub use from as mock derive with
mut cap comptime if else match while for in loop return break continue assert except or
true false is self Self it old spawn scope await select race
requires ensures holds invariant decreases where assumed by
assume calc simplify once unfold witness arithmetic nonlinear bits compute auto""".split())

STRATA_TOKEN = re.compile(
    r'(?P<comment>//[^\n]*)'
    r'|(?P<string>[rf]?"(?:\\.|[^"\\\n])*"?)'
    r'|(?P<number>\b(?:0x[0-9a-fA-F_]+|0b[01_]+|\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?)\b)'
    r'|(?P<word>\b[A-Za-z_][A-Za-z0-9_]*\b)'
    r'|(?P<other>[\s\S])'
)


def highlight_strata(code):
    out = []
    for m in STRATA_TOKEN.finditer(code):
        kind, text = m.lastgroup, m.group()
        if kind == "word":
            if text in KEYWORDS:
                kind = "kw"
            elif text[0].isupper() or re.fullmatch(r"[su]\d+|f16|f32|f64|bf16", text):
                kind = "ty"
            elif code[m.end() : m.end() + 1] == "(":
                kind = "fn"
            else:
                kind = None
        elif kind == "other":
            kind = None
        piece = html.escape(text, quote=False)
        out.append(f'<span class="{kind}">{piece}</span>' if kind else piece)
    return "".join(out)


def inline(text):
    parts = re.split(r"(`+)(.+?)\1", text)
    out = []
    i = 0
    while i < len(parts):
        plain = html.escape(parts[i], quote=False)
        plain = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", plain)
        plain = re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r'<a href="\2">\1</a>', plain)
        out.append(plain)
        if i + 2 < len(parts):
            out.append("<code>" + html.escape(parts[i + 2], quote=False) + "</code>")
        i += 3
    return "".join(out)


def table(rows):
    cells = [
        [c.strip().replace("\\|", "|") for c in re.split(r"(?<!\\)\|", r.strip()[1:-1])]
        for r in rows
    ]
    head, body = cells[0], cells[2:]
    out = ['<div class="scroll"><table class="stack"><thead><tr>']
    out += ["<th>" + inline(c) + "</th>" for c in head]
    out.append("</tr></thead><tbody>")
    for r in body:
        out.append("<tr>" + "".join("<td>" + inline(c) + "</td>" for c in r) + "</tr>")
    out.append("</tbody></table></div>")
    return "".join(out)


def lists(lines):
    items = []
    for line in lines:
        m = re.match(r"( *)(- |\d+\. )(.*)", line)
        if m:
            tag = "ul" if m.group(2) == "- " else "ol"
            items.append([len(m.group(1)) // 2, tag, m.group(3)])
        else:
            items[-1][2] += " " + line.strip()
    return render_items(items, 0)[0]


def render_items(items, start):
    level, tag = items[start][0], items[start][1]
    out = ["<" + tag + ">"]
    i = start
    while i < len(items) and items[i][0] >= level:
        if items[i][0] > level:
            inner, i = render_items(items, i)
            out[-1] = out[-1][: -len("</li>")] + inner + "</li>"
            continue
        out.append("<li>" + inline(items[i][2]) + "</li>")
        i += 1
    out.append("</" + tag + ">")
    return "".join(out), i


def body(md):
    lines = md.split("\n")
    out = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("```"):
            lang = line[3:].strip()
            j = i + 1
            while not lines[j].startswith("```"):
                j += 1
            raw = "\n".join(lines[i + 1 : j])
            code = highlight_strata(raw) if lang == "strata" else html.escape(raw, quote=False)
            out.append(f'<pre class="{lang or "text"}"><code>{code}</code></pre>')
            i = j + 1
        elif line.startswith("#"):
            level = len(line) - len(line.lstrip("#"))
            text = line[level:].strip()
            m = re.match(r"(\d+(?:\.\d+)?)\.? ", text)
            anchor = f' id="s{m.group(1).replace(".", "-")}"' if m else ""
            out.append(f"<h{level}{anchor}>{inline(text)}</h{level}>")
            i += 1
        elif line.startswith("|"):
            j = i
            while j < len(lines) and lines[j].startswith("|"):
                j += 1
            out.append(table(lines[i:j]))
            i = j
        elif re.match(r"(- |\d+\. )", line):
            j = i
            while j < len(lines) and lines[j].strip() and (
                re.match(r" *(- |\d+\. )", lines[j]) or lines[j].startswith("  ")
            ):
                j += 1
            out.append(lists(lines[i:j]))
            i = j
        elif line.strip():
            j = i
            while j < len(lines) and lines[j].strip() and not re.match(r"(#|```|\||- |\d+\. )", lines[j]):
                j += 1
            out.append("<p>" + inline(" ".join(lines[i:j])) + "</p>")
            i = j
        else:
            i += 1
    return "\n".join(out)


def strip(md):
    written = []
    for n, (name, state) in enumerate(SECTIONS, 1):
        cls = "draft" if state == "draft" else "wait" if state.startswith("waits") else "todo"
        link = f'href="#s{n}"' if state == "draft" else ""
        written.append(
            f'<a class="sec {cls}" {link}><b>{n}</b><span>{html.escape(name)}</span>'
            f"<i>{html.escape(state)}</i></a>"
        )
    return "".join(written)


STYLE = """
/* VOICE a reference document: calm, exact, built to be read end to end
   IDEA a standards document, numbered sections and ruled tables
   PICTURE the twelve sections as a strip, each marked drafted, next or waiting
   COLOUR white ground, near-black ink, one blue for links and drafted sections
   TYPE Figtree for text and headings, JetBrains Mono for code
   LAYOUT title and status across the top, the strip beneath, then one text column */
:root{color-scheme:light dark;--bg:light-dark(#ffffff,#0e1116);--ink:light-dark(#15171c,#e7e9ee);--soft:light-dark(#5b6170,#9aa1b1);--rule:light-dark(#dfe2e8,#2a2f3a);--code:light-dark(#f4f5f8,#171b23);--blue:light-dark(#0a5cff,#6f9dff);--wait:light-dark(#fff4d6,#2c2510)}
:root[data-theme=light]{color-scheme:light}:root[data-theme=dark]{color-scheme:dark}
*{box-sizing:border-box;min-width:0}
body{margin:0;background:var(--bg);color:var(--ink);font:16.5px/1.6 Figtree,system-ui,sans-serif;overflow-x:clip}
.band{padding:clamp(28px,4vw,56px) max(5vw,16px)}
header{border-bottom:1px solid var(--rule)}
h1{font-size:clamp(32px,5vw,48px);line-height:1.05;letter-spacing:-.02em;margin:0 0 12px;text-wrap:balance}
.lede{max-width:68ch;color:var(--soft);margin:0 0 24px}
.strip{display:grid;grid-template-columns:repeat(auto-fit,minmax(9.5em,1fr));gap:6px}
.sec{display:grid;gap:2px;padding:10px 12px;border:1px solid var(--rule);border-radius:6px;color:var(--ink);text-decoration:none}
.sec b{font-size:22px;line-height:1}
.sec span{font-weight:600;font-size:14.5px}
.sec i{font-style:normal;font-size:13px;color:var(--soft)}
.sec.draft{background:var(--blue);border-color:var(--blue);color:#fff}
.sec.draft i{color:#fff}
.sec.wait{background:var(--wait)}
main{max-width:860px}
h2{font-size:32px;line-height:1.1;letter-spacing:-.02em;margin:48px 0 12px;padding-top:16px;border-top:2px solid var(--ink)}
h3{font-size:20px;margin:36px 0 8px}
p,li{max-width:70ch}
a{color:var(--blue)}
code{font:14px/1.5 "JetBrains Mono",ui-monospace,monospace;overflow-wrap:anywhere}
:not(pre)>code{background:var(--code);padding:1px 4px;border-radius:3px}
pre{background:var(--code);padding:14px 16px;border-radius:6px;overflow-x:auto}
pre code{overflow-wrap:normal;white-space:pre}
.scroll{overflow-x:auto}
pre .kw{color:light-dark(#a3159b,#e08ae0);font-weight:600}
pre .ty{color:light-dark(#0a5cff,#7fb0ff)}
pre .fn{color:light-dark(#0b6e4f,#5fd3a6)}
pre .string{color:light-dark(#a14a00,#f0a868)}
pre .number{color:light-dark(#8a5a00,#e5c07b)}
pre .comment{color:light-dark(#6b7280,#7d8596);font-style:italic}
table{border-collapse:collapse;margin:12px 0 20px;font-size:15px}
th,td{text-align:left;vertical-align:top;padding:7px 12px 7px 0;border-bottom:1px solid var(--rule)}
th{font-weight:700;border-bottom:2px solid var(--ink)}
html{scroll-padding-top:24px}
@media(max-width:600px){table{font-size:13px}.stack thead{display:none}.stack tr{display:grid;padding-block:10px;border-bottom:1px solid var(--rule)}.stack td{display:flow-root;border:0;padding:3px 0;padding-inline-start:8em}.stack td::before{content:attr(data-l);float:inline-start;width:7.5em;margin-inline-start:-8em;color:var(--soft)}.stack td:empty{display:none}}
"""

SCRIPT = """<script>{new MutationObserver(()=>{for(const c of document.querySelectorAll('.stack td:not([data-l])')){let i=0,p=c;while(p=p.previousElementSibling)i+=p.colSpan;c.dataset.l=c.closest('table').rows[0].cells[i]?.textContent||''}}).observe(document,{childList:true,subtree:true})}</script>"""


def page(md):
    title_line, rest = md.split("\n", 1)
    intro, sections = rest.split("\n## ", 1)
    return f"""<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Strata language spec</title>
<link rel="preconnect" href="https://fonts.googleapis.com"><link href="https://fonts.googleapis.com/css2?family=Figtree:wght@400;600;700&family=JetBrains+Mono:wght@400&display=swap" rel="stylesheet">
<style>{STYLE}</style>{SCRIPT}</head><body>
<header class="band"><h1>{inline(title_line.lstrip("# "))}</h1>
<div class="lede">{body(intro)}</div>
<nav class="strip">{strip(md)}</nav></header>
<main class="band">{body("## " + sections)}</main>
</body></html>
"""


if __name__ == "__main__":
    with open(sys.argv[1]) as f:
        md = f.read()
    with open(sys.argv[2], "w") as f:
        f.write(page(md))
