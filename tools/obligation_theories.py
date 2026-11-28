#!/usr/bin/env python3
"""Which theories the solver's queries use, and what each obligation cost.

The input is one folder per module, RUN/PACKAGE/MODULE, holding what
    STRATA_SOLVER_DUMP=DIR/dump STRATA_SOLVER_LOG=DIR/solver.log \\
      strata verify 'MODULE.*' --no-cache --no-race --cross-check 0 --json --checks > DIR/verify.json
writes (a function's batch and single queries start with `; fn NAME` in the
dump), and DIR/done.tsv (package, module, exit code, seconds, peak memory).

    obligation_theories.py collect RUN OUT.tsv [JOBS]   a row per obligation, each
        query also asked alone (`z3 -T:20 rlimit=2000000 -st`, the budget of a
        single query in compiler/smt)
    obligation_theories.py summary OUT.tsv MODULES.tsv [RUN]   the tables, as markdown

A query asked alone is timed from outside z3, less the time z3 takes to start
and answer `(check-sat)`.

Only what a query's assertions reach counts towards its theories: the
definitions they name and their bodies, and the sorts of the unknowns and
functions they name.
"""
import json
import os
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor

ARITH = {"+", "-", "*", "/", "div", "mod", "abs", "<", "<=", ">", ">=", "to_real", "to_int", "is_int"}
SPLITS = {"ite", "or", "=>", "xor"}
ARRAY = {"select", "store"}


def tokens(text):
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c in " \t\r\n":
            i += 1
        elif c == ";":
            while i < n and text[i] != "\n":
                i += 1
        elif c in "()":
            yield c
            i += 1
        elif c == '"':
            j = i + 1
            while j < n:
                if text[j] == '"':
                    if j + 1 < n and text[j + 1] == '"':
                        j += 2
                        continue
                    break
                j += 1
            yield ("str", text[i:j + 1])
            i = j + 1
        elif c == "|":
            j = text.index("|", i + 1)
            yield text[i:j + 1]
            i = j + 1
        else:
            j = i
            while j < n and text[j] not in " \t\r\n()|\";":
                j += 1
            yield text[i:j]
            i = j


def parse(text):
    stack = [[]]
    for t in tokens(text):
        if t == "(":
            stack.append([])
        elif t == ")":
            top = stack.pop()
            stack[-1].append(top)
        else:
            stack[-1].append(t)
    return stack[0]


def is_num(t):
    return isinstance(t, str) and re.fullmatch(r"\d+(\.\d+)?", t) is not None




def classify_script(text):
    cmds = parse(text)
    defs = {}
    datatypes = {}
    ctor = {}
    sorts = set()
    funs = {}
    asserts = []
    rec = set()
    for c in cmds:
        if not isinstance(c, list) or not c:
            continue
        h = c[0]
        if h == "assert":
            asserts.append(c[1])
        elif h == "define-fun":
            defs[c[1]] = c
        elif h in ("define-fun-rec",):
            defs[c[1]] = c
            rec.add(c[1])
        elif h == "define-funs-rec":
            for decl, body in zip(c[1], c[2]):
                defs[decl[0]] = ["define-fun", decl[0], decl[1], decl[2], body]
                rec.add(decl[0])
        elif h == "declare-fun":
            funs[c[1]] = c
        elif h == "declare-const":
            funs[c[1]] = ["declare-fun", c[1], [], c[2]]
        elif h == "declare-sort":
            sorts.add(c[1])
        elif h == "declare-datatypes":
            for (name, _), cs in zip(c[1], c[2]):
                datatypes[name] = cs
                for k in cs:
                    kn = k[0] if isinstance(k, list) else k
                    ctor[kn] = name
                    if isinstance(k, list):
                        for f in k[1:]:
                            ctor[f[0]] = name
    feats = set()
    reached = set()
    work = list(asserts)
    sort_work = []

    def const_term(t):
        if is_num(t):
            return True
        if isinstance(t, str) and t in defs and defs[t][2] == []:
            return const_term(defs[t][4])
        if isinstance(t, list) and t and t[0] in ("-", "+", "*", "/") and all(const_term(x) for x in t[1:]):
            return True
        return False

    def visit(t, bound):
        if isinstance(t, tuple):
            feats.add("string-literals")
            return
        if isinstance(t, str):
            if is_num(t) and "." in t:
                feats.add("real")
            if t.startswith("#b") or t.startswith("#x"):
                feats.add("bitvectors")
            if t in bound:
                return
            use(t)
            return
        if not t:
            return
        h = t[0]
        if h in ("forall", "exists"):
            feats.add("quantifiers")
            b = set(bound)
            for v in t[1]:
                b.add(v[0])
                sort_work.append(v[1])
            body = t[2]
            if isinstance(body, list) and body and body[0] == "!":
                if ":pattern" in [x for x in body if isinstance(x, str)]:
                    feats.add("patterns")
                visit(body[1], b)
                for k, x in enumerate(body):
                    if x == ":pattern":
                        visit(body[k + 1], b)
            else:
                visit(body, b)
            return
        if h == "lambda":
            feats.add("lambda")
            b = set(bound)
            for v in t[1]:
                b.add(v[0])
                sort_work.append(v[1])
            visit(t[2], b)
            return
        if h == "let":
            b = set(bound)
            for v in t[1]:
                visit(v[1], bound)
                b.add(v[0])
            visit(t[2], b)
            return
        if h == "!":
            visit(t[1], bound)
            return
        if h == "as":
            sort_work.append(t[2])
            visit(t[1], bound) if isinstance(t[1], list) else None
            if t[1] == "const":
                feats.add("arrays")
            return
        if isinstance(h, list):
            if h and h[0] == "_" and h[1] == "is":
                feats.add("datatypes")
            elif h and h[0] == "as" and h[1] == "const":
                feats.add("arrays")
            elif h and h[0] == "_":
                op = h[1]
                if op.startswith("bv") or op in ("extract", "zero_extend", "sign_extend", "rotate_left", "rotate_right", "repeat", "int2bv", "bv2nat", "bv2int"):
                    feats.add("bitvectors")
                elif op in ("re.loop", "re.^", "char"):
                    feats.add("regex")
            for x in t[1:]:
                visit(x, bound)
            return
        if h == "_":
            if len(t) > 1 and isinstance(t[1], str) and t[1].startswith("bv"):
                feats.add("bitvectors")
            if t[1] == "re.allchar" or t[1] == "char":
                feats.add("regex")
            return
        if h in ARITH:
            feats.add("arith")
            args = t[1:]
            if h == "*" and sum(0 if const_term(a) else 1 for a in args) >= 2:
                feats.add("nonlinear")
            if h in ("div", "mod", "/") and len(args) == 2 and not const_term(args[1]):
                feats.add("nonlinear")
            if h in ("div", "mod"):
                feats.add("divmod-const" if "nonlinear" not in feats or const_term(args[-1]) else "divmod")
            if h in ("/", "to_real", "to_int", "is_int"):
                feats.add("real")
        elif h in SPLITS:
            feats.add("splits")
        elif h == "not" and isinstance(t[1], list) and t[1] and t[1][0] == "and":
            feats.add("splits")
        elif h in ARRAY:
            feats.add("arrays")
        elif isinstance(h, str) and (h.startswith("str.") or h.startswith("re.")):
            feats.add("string-lengths" if h == "str.len" else "regex" if h.startswith("re.") or h in ("str.in_re", "str.to_re") else "string-ops")
        elif isinstance(h, str) and h.startswith("seq."):
            feats.add("sequences")
        elif isinstance(h, str) and (h.startswith("bv") or h in ("concat", "extract")):
            feats.add("bitvectors")
        elif h in ("^", "exp", "power"):
            feats.add("nonlinear")
        if isinstance(h, str) and h not in bound:
            use(h)
        for x in t[1:]:
            visit(x, bound)

    def use(name):
        if name in reached:
            return
        reached.add(name)
        if name in ctor:
            feats.add("datatypes")
        if name in defs:
            d = defs[name]
            if name in rec:
                feats.add("recursive-defs")
            for p in d[2]:
                sort_work.append(p[1])
            sort_work.append(d[3])
            visit(d[4], {p[0] for p in d[2]})
        elif name in funs:
            f = funs[name]
            if f[2]:
                feats.add("uf")
            for s in f[2]:
                sort_work.append(s)
            sort_work.append(f[3])

    while work:
        visit(work.pop(), set())
    seen_sorts = set()
    while sort_work:
        s = sort_work.pop()
        if isinstance(s, list):
            if s and s[0] == "Array":
                feats.add("arrays")
            if s and s[0] == "_" and s[1] == "BitVec":
                feats.add("bitvectors")
            if s and s[0] == "Seq":
                feats.add("sequences")
            sort_work.extend(s[1:] if s and s[0] != "_" else [])
            continue
        if s in seen_sorts:
            continue
        seen_sorts.add(s)
        if s == "String" or s == "RegLan":
            feats.add("string-sort")
        elif s == "Real":
            feats.add("real")
        elif s in sorts:
            feats.add("uninterpreted-sorts")
        elif s in datatypes:
            feats.add("datatypes")
            for k in datatypes[s]:
                if isinstance(k, list):
                    for f in k[1:]:
                        sort_work.append(f[1])
    return feats, len(asserts)


ORDER = ["arith", "real", "splits", "uf", "uninterpreted-sorts", "datatypes", "arrays", "string-sort", "string-literals",
         "string-lengths", "string-ops", "regex",
         "sequences", "bitvectors", "nonlinear", "quantifiers", "patterns", "lambda", "recursive-defs"]


def theory_class(feats):
    if "quantifiers" in feats or "lambda" in feats or "recursive-defs" in feats:
        top = "quantifiers"
    elif "nonlinear" in feats:
        top = "nonlinear"
    elif feats & {"string-ops", "regex", "sequences"}:
        top = "strings"
    elif "string-lengths" in feats:
        top = "string-lengths"
    elif "bitvectors" in feats:
        top = "bitvectors"
    elif "arrays" in feats:
        top = "arrays"
    elif "datatypes" in feats:
        top = "datatypes"
    elif feats & {"uf", "uninterpreted-sorts", "string-sort", "string-literals"}:
        top = "uf"
    elif "splits" in feats:
        top = "linear+splits"
    else:
        top = "linear"
    return top


def norm(s):
    return re.sub(r"\s+", " ", s).strip()


def read_dumps(dump):
    out = []
    for name in sorted(os.listdir(dump)):
        if not name.endswith(".smt2"):
            continue
        text = open(os.path.join(dump, name), encoding="utf-8").read()
        lines = text.split("\n")
        end = next((i for i, l in enumerate(lines) if l.startswith("; answer:")), len(lines))
        script, tail = lines[:end], lines[end:]
        outs = [l[7:] for l in tail if l.startswith("; out: ")]
        fn = next((l[5:] for l in script if l.startswith("; fn ")), None)
        out.append((name[:-5], fn, script, outs))
    return out


def answers(outs):
    chunks, cur = [], []
    for o in outs:
        if o == "@@answered":
            chunks.append(cur)
            cur = []
        else:
            cur.append(o)
    if cur:
        chunks.append(cur)
    res = []
    last = 0
    for ch in chunks:
        a = ch[0] if ch else "none"
        units = ""
        for o in ch:
            m = re.match(r"\(:rlimit (\d+)\)", o)
            if m:
                u = int(m.group(1))
                units = str(u - last if u >= last else u)
                last = u
        res.append((a, units))
    return res


def queries(script):
    prefix, block, inside, qs = [], [], False, []
    for l in script:
        if l == "(push)":
            inside, block = True, []
        elif l == "(pop)":
            inside = False
            what = norm(" ".join(x[2:] for x in block if x.startswith("; ")))
            body = [x for x in block if not x.startswith("(get-") and not x.startswith(";")]
            keep = [p for p in prefix if not p.startswith("(set-option :print-success")]
            qs.append((what, "\n".join(keep + body) + "\n"))
        elif inside:
            block.append(l)
        elif l.startswith("(") and not l.startswith("(echo") and not l.startswith("(get-"):
            prefix.append(l)
    return qs


def single_what(script):
    return norm(" ".join(l[2:] for l in script if l.startswith("; ") and not l.startswith("; fn ")))


def run_alone(path):
    t0 = time.monotonic()
    try:
        p = subprocess.run(["z3", "-T:20", "rlimit=2000000", "-st", path], capture_output=True, text=True, timeout=40)
        out = p.stdout
    except subprocess.TimeoutExpired:
        out = "timeout"
    wall = time.monotonic() - t0
    first = next((l.strip() for l in out.split("\n") if l.strip() and not l.startswith("(:") and not l.startswith("(error")), "")
    units = re.search(r":rlimit-count\s+(\d+)", out)
    return first or "error", units.group(1) if units else "", f"{wall:.4f}"


def outcome(a):
    if a.startswith("(error"):
        return "error"
    return {"unsat": "proven", "sat": "refuted", "unknown": "unknown", "timeout": "timeout"}.get(a, a or "none")


COLS = ["package", "module", "fn", "kind", "closed_by", "class", "theories", "asserts", "outcome",
        "lab_answer", "lab_units", "alone_answer", "alone_units", "alone_secs", "query", "what"]


def collect(run, out_path, jobs=4):
    rows = []
    work = []
    tmp = os.path.join(run, "_queries")
    os.makedirs(tmp, exist_ok=True)
    for pkg in sorted(os.listdir(run)):
        if pkg.startswith("_") or not os.path.isdir(os.path.join(run, pkg)):
            continue
        for mod in sorted(os.listdir(os.path.join(run, pkg))):
            d = os.path.join(run, pkg, mod)
            if not os.path.isdir(os.path.join(d, "dump")):
                continue
            asked = {}
            singles = {}
            mod_rows = []
            for h, fn, script, outs in read_dumps(os.path.join(d, "dump")):
                if fn is not None and "(push)" in script:
                    for (what, body), (a, u) in zip(queries(script), answers(outs) + [("none", "")] * 10000):
                        mod_rows.append({"package": pkg, "module": mod, "fn": fn, "kind": "check", "closed_by": "solver",
                                         "lab_answer": a, "lab_units": u, "what": what, "_body": body})
                        asked[(fn, what)] = asked.get((fn, what), 0) + 1
                elif fn is not None:
                    a = (answers(outs) or [("none", "")])[0]
                    singles.setdefault((fn, single_what(script)), []).append(a)
                else:
                    a, u = (answers(outs) or [("none", "")])[0]
                    mod_rows.append({"package": pkg, "module": mod, "fn": "-", "kind": "other", "closed_by": "solver",
                                     "lab_answer": a, "lab_units": u, "what": "-",
                                     "_body": "\n".join(l for l in script if not l.startswith("(set-option :print-success")) + "\n"})
            for r in mod_rows:
                s = singles.get((r["fn"], r["what"]))
                if r["kind"] == "check" and s:
                    a, u = s.pop(0)
                    r["lab_answer"], r["lab_units"], r["kind"] = a, u, "check+single"
            try:
                verdicts = json.load(open(os.path.join(d, "verify.json")))
            except (ValueError, OSError):
                verdicts = []
            names = {v["fn"] for v in verdicts}
            for r in mod_rows:
                if r["fn"] != "-" and r["fn"] not in names:
                    found = [n for n in names if n.endswith("." + r["fn"])]
                    if len(found) == 1:
                        r["fn"] = found[0]
            asked = {}
            for r in mod_rows:
                if r["kind"] != "other":
                    asked[(r["fn"], r["what"])] = asked.get((r["fn"], r["what"]), 0) + 1
            for v in verdicts:
                for c in v.get("verdict", {}).get("checks", []) or []:
                    key = (v["fn"], norm(c))
                    if asked.get(key, 0) > 0:
                        asked[key] -= 1
                    else:
                        mod_rows.append({"package": pkg, "module": mod, "fn": v["fn"], "kind": "check",
                                         "closed_by": "probe" if norm(c).endswith("(derived)") else "none",
                                         "class": "-", "theories": "-", "asserts": "-", "outcome": "proven",
                                         "lab_answer": "-", "lab_units": "-", "alone_answer": "-", "alone_units": "-",
                                         "alone_secs": "-", "what": norm(c)})
            for r in mod_rows:
                if "_body" in r:
                    p = os.path.join(tmp, f"q{len(work):06d}.smt2")
                    with open(p, "w") as f:
                        f.write(r["_body"])
                    feats, n = classify_script(r.pop("_body"))
                    r["theories"] = "+".join(x for x in ORDER if x in feats) or "none"
                    r["class"] = theory_class(feats)
                    r["asserts"] = str(n)
                    r["outcome"] = outcome(r["lab_answer"]) if r["kind"] != "other" else r["lab_answer"]
                    r["query"] = os.path.basename(p)
                    work.append((r, p))
            rows += mod_rows
    empty = os.path.join(tmp, "empty.smt2")
    with open(empty, "w") as f:
        f.write("(check-sat)\n")
    with ThreadPoolExecutor(int(jobs)) as ex:
        base = sorted(float(x[2]) for x in ex.map(lambda _: run_alone(empty), range(40)))[20]
        print(f"z3 started and answered `(check-sat)` in {base:.4f} s (the median of 40), taken off each time", file=sys.stderr)
        for (r, p), (a, u, s) in zip(work, ex.map(lambda w: run_alone(w[1]), work)):
            r["alone_answer"], r["alone_units"], r["alone_secs"] = a, u, f"{max(0.0, float(s) - base):.4f}"
    with open(out_path, "w") as out:
        out.write("\t".join(COLS) + "\n")
        for r in rows:
            out.write("\t".join(str(r.get(c, "")).replace("\t", " ") for c in COLS) + "\n")


ROUGH = ["linear", "linear+splits", "uf", "datatypes", "arrays", "string-lengths", "strings", "bitvectors", "nonlinear", "quantifiers"]


def pct(a, b):
    return f"{100.0 * a / b:.0f}%" if b else "-"


def quantile(xs, q):
    if not xs:
        return 0.0
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(q * len(xs)))]


def summary(path, modules_path, run=None):
    rows = [dict(zip(COLS, l.split("\t"))) for l in open(path).read().split("\n")[1:] if l]
    pkgs = sorted({r["package"] for r in rows}, key=lambda p: ["core", "std", "compiler"].index(p) if p in ("core", "std", "compiler") else 9)
    obl = [r for r in rows if r["kind"] != "other"]
    print("Obligations by rung (`check` rows: the obligations of each function's batch, and those closed with no query):\n")
    print("| package | obligations | no solver (lab) | derived invariants | solver | solver: proven | unknown | refuted | solver error |")
    print("|---|---|---|---|---|---|---|---|---|")
    for p in pkgs + ["all"]:
        rs = [r for r in obl if p == "all" or r["package"] == p]
        sv = [r for r in rs if r["closed_by"] == "solver"]
        no = [r for r in rs if r["closed_by"] == "none"]
        pr = [r for r in rs if r["closed_by"] == "probe"]
        c = lambda o: sum(1 for r in sv if r["outcome"] == o)
        print(f"| {p} | {len(rs)} | {len(no)} ({pct(len(no), len(rs))}) | {len(pr)} | {len(sv)} | {c('proven')} | {c('unknown')} | {c('refuted')} | {len(sv) - c('proven') - c('unknown') - c('refuted')} |")
    print("\nSolver obligations by the richest theory their query uses (each class includes the ones above it; `linear`: linear integer/real arithmetic, a conjunction):\n")
    head = "| class | " + " | ".join(pkgs) + " | all | share | of solver time | median s | p90 s | max s | median units | p90 units |"
    print(head)
    print("|" + "---|" * (head.count("|") - 1))
    sv_all = [r for r in obl if r["closed_by"] == "solver"]
    total_t = sum(float(r["alone_secs"] or 0) for r in sv_all)
    for k in ROUGH:
        rs = [r for r in sv_all if r["class"] == k]
        ts = [float(r["alone_secs"] or 0) for r in rs]
        us = [int(r["alone_units"]) for r in rs if r["alone_units"].isdigit()]
        per = " | ".join(str(sum(1 for r in rs if r["package"] == p)) for p in pkgs)
        print(f"| {k} | {per} | {len(rs)} | {pct(len(rs), len(sv_all))} | {pct(sum(ts), total_t)} | {quantile(ts, .5):.3f} | {quantile(ts, .9):.3f} | {max(ts or [0]):.2f} | {quantile(us, .5):.0f} | {quantile(us, .9):.0f} |")
    print("\nTheory combinations (solver obligations, the 25 most common):\n")
    print("| theories | " + " | ".join(pkgs) + " | all |")
    print("|---|" + "---|" * (len(pkgs) + 1))
    combos = {}
    for r in sv_all:
        combos.setdefault(r["theories"], []).append(r)
    for t, rs in sorted(combos.items(), key=lambda x: -len(x[1]))[:25]:
        print(f"| {t} | " + " | ".join(str(sum(1 for r in rs if r["package"] == p)) for p in pkgs) + f" | {len(rs)} |")
    other = [r for r in rows if r["kind"] == "other"]
    print(f"\nOther solver scripts (no `; fn` label: termination bounds, invariant and refinement probes, counterexample inputs): {len(other)}")
    print("\n| class | " + " | ".join(pkgs) + " | all |")
    print("|---|" + "---|" * (len(pkgs) + 1))
    for k in ROUGH:
        rs = [r for r in other if r["class"] == k]
        print(f"| {k} | " + " | ".join(str(sum(1 for r in rs if r["package"] == p)) for p in pkgs) + f" | {len(rs)} |")
    print("\nOwn procedures: the share of solver obligations (and of their time asked alone) each set of theories covers:\n")
    print("| covered | obligations | proven ones | time |")
    print("|---|---|---|---|")
    proven = [r for r in sv_all if r["outcome"] == "proven"]
    for name, ks in [("linear arithmetic, a conjunction", ["linear"]),
                     ("+ case splits", ["linear", "linear+splits"]),
                     ("+ uninterpreted functions", ["linear", "linear+splits", "uf"]),
                     ("+ datatypes", ["linear", "linear+splits", "uf", "datatypes"]),
                     ("+ arrays", ["linear", "linear+splits", "uf", "datatypes", "arrays"]),
                     ("+ string lengths", ["linear", "linear+splits", "uf", "datatypes", "arrays", "string-lengths"]),
                     ("+ string operations, regular expressions", ["linear", "linear+splits", "uf", "datatypes", "arrays", "string-lengths", "strings"]),
                     ("+ bitvectors, nonlinear", ["linear", "linear+splits", "uf", "datatypes", "arrays", "string-lengths", "strings", "bitvectors", "nonlinear"])]:
        rs = [r for r in sv_all if r["class"] in ks]
        pr = [r for r in proven if r["class"] in ks]
        t = sum(float(r["alone_secs"] or 0) for r in rs)
        print(f"| {name} | {len(rs)} ({pct(len(rs), len(sv_all))}) | {len(pr)} ({pct(len(pr), len(proven))}) | {pct(t, total_t)} |")
    if run:
        print("\nThe lab's own run (STRATA_SOLVER_LOG: scripts asked, not answered again from the run's memo):\n")
        print("| package | scripts | batches and singles | other | solver seconds | of it in batches and singles |")
        print("|---|---|---|---|---|---|")
        for p in pkgs:
            n = nb = 0
            t = tb = 0.0
            for mod in sorted(os.listdir(os.path.join(run, p))):
                d = os.path.join(run, p, mod)
                labelled = set()
                for h, fn, script, outs in read_dumps(os.path.join(d, "dump")):
                    if fn is not None:
                        labelled.add(h)
                for l in open(os.path.join(d, "solver.log")).read().split("\n") if os.path.exists(os.path.join(d, "solver.log")) else []:
                    f = l.split("\t")
                    if len(f) < 5 or l.endswith("\tagain"):
                        continue
                    n += 1
                    t += float(f[0])
                    if f[3] in labelled:
                        nb += 1
                        tb += float(f[0])
            print(f"| {p} | {n} | {nb} | {n - nb} | {t:.1f} | {pct(tb, t)} |")
    print("\nModules:\n")
    print("| package | module | exit | seconds | peak MB | obligations | no solver | solver | other scripts |")
    print("|---|---|---|---|---|---|---|---|---|")
    for l in open(modules_path).read().split("\n"):
        if not l:
            continue
        p, m, code, secs, rss = (l.split("\t") + [""] * 5)[:5]
        rs = [r for r in rows if r["package"] == p and r["module"] == m]
        o = [r for r in rs if r["kind"] != "other"]
        s = [r for r in o if r["closed_by"] == "solver"]
        mb = f"{int(rss) / 1e6:.0f}" if rss.isdigit() else "-"
        flag = " (time limit)" if float(secs or 0) >= 299 else ""
        print(f"| {p} | {m} | {code}{flag} | {float(secs or 0):.0f} | {mb} | {len(o)} | {len(o) - len(s)} | {len(s)} | {len(rs) - len(o)} |")


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "collect":
        collect(sys.argv[2], sys.argv[3], *(sys.argv[4:5] or [4]))
    elif cmd == "summary":
        summary(sys.argv[2], sys.argv[3], *(sys.argv[4:5]))
