# Runs st.strata under `strata run --live` through a scripted sequence of
# edits and commands, and checks every state line against a model of what
# the code running at that moment computes.
import subprocess, sys, time, re, os, copy

S = sys.argv[1]
MODE = sys.argv[2] if len(sys.argv) > 2 else "live"   # live | plain-base | plain-final
BASE = open("base.st").read()

def defs_of(text):
    """The base text cut into top-level definitions (with their docs)."""
    out, cur = {}, []
    for line in text.split("\n"):
        cur.append(line)
        if line == "}" or (line.startswith("type Rec")):
            body = "\n".join(cur).strip("\n")
            m = re.search(r"^(fn|type) (\w+)", body, re.M)
            out[m.group(2)] = body + "\n"
            cur = []
    return out

BASEDEFS = defs_of(BASE)

P0 = dict(bump=1, key_mod=3, key_prefix="k", add_mul=1, name_prefix="r",
          score="plain", score_add=1, grow=1, keep_lim=5, label="a",
          scorer_add=0, fold_mul=1, worker_add=0, inner_add=100, drop7=False)

def text(name, P):
    if name == "bump":
        return f"fn bump() -> Int {{\n  {P['bump']}\n}}\n"
    if name == "key_of":
        return f'fn key_of(t: Int) -> String {{\n  "{P["key_prefix"]}" + to_string(t % {P["key_mod"]})\n}}\n'
    if name == "add_of":
        return f"fn add_of(t: Int) -> Int {{\n  t * {P['add_mul']}\n}}\n"
    if name == "name_of":
        return f'fn name_of(t: Int) -> String {{\n  "{P["name_prefix"]}" + to_string(t)\n}}\n'
    if name == "score_of":
        if P["score"] == "plain":
            return f"fn score_of(r: Rec, t: Int) -> Int {{\n  r.score + {P['score_add']} + (t - t)\n}}\n"
        return "fn score_of(r: Rec, t: Int) -> Int {\n  grow(r.score) + (t - t)\n}\n"
    if name == "grow":
        return f"/// `x` grown.\nfn grow(x: Int) -> Int {{\n  x + {P['grow']}\n}}\n"
    if name == "keep":
        return f"fn keep(r: Rec) -> Bool {{\n  r.score < {P['keep_lim']}\n}}\n"
    if name == "label":
        return f'fn label(t: Int) -> String {{\n  "{P["label"]}" + to_string(t - t)\n}}\n'
    if name == "make_scorer":
        return f"fn make_scorer(k: Int) -> (Int) -> Int {{\n  x -> x * k + {P['scorer_add']}\n}}\n"
    if name == "fold_in":
        return f"fn fold_in(acc: Int, x: Int) -> Int {{\n  acc + x * {P['fold_mul']}\n}}\n"
    if name == "step":
        return BASEDEFS["step"].replace("r -> keep(r)", "r -> keep(r) && r.id % 7 != 0" if P["drop7"] else "r -> keep(r)")
    if name == "worker":
        return BASEDEFS["worker"].replace("fold_in(acc, x) + 0", f"fold_in(acc, x) + {P['worker_add']}")
    if name == "inner_loop":
        return BASEDEFS["inner_loop"].replace("s.count += 100", f"s.count += {P['inner_add']}")
    return BASEDEFS[name]

PARAM_DEFS = {"bump": ["bump"], "key_mod": ["key_of"], "key_prefix": ["key_of"],
              "add_mul": ["add_of"], "name_prefix": ["name_of"], "score": ["score_of", "grow"],
              "score_add": ["score_of"], "grow": ["grow"], "keep_lim": ["keep"], "label": ["label"],
              "scorer_add": ["make_scorer"], "fold_mul": ["fold_in"], "worker_add": ["worker"],
              "inner_add": ["inner_loop"], "drop7": ["step"]}

def program(P):
    names = [n for n in BASEDEFS]
    body = "from std.io use Terminal\n\n" + "\n".join(text(n, P) for n in names)
    if P["score"] != "plain":
        body += "\n" + text("grow", P)
    return body

class Model:
    def __init__(self, P):
        self.P = P                      # the code running now
        self.count = 0; self.table = {}; self.recs = []; self.log = []
        self.scored = 0; self.acc = 0; self.t = 0
        self.scorer = (2, P["scorer_add"])      # captured by make_scorer(2)
        self.worker_add = P["worker_add"]       # the worker's frame lasts the run
        self.inner_add = None                   # inside inner_loop: its frame's
    def score(self, r):
        P = self.P
        if P["score"] == "plain":
            return r[2] + P["score_add"]
        return r[2] + P["grow"]
    def step(self):
        P = self.P; t = self.t
        self.count += P["bump"]
        k = P["key_prefix"] + str(t % P["key_mod"])
        self.table[k] = self.table.get(k, 0) + t * P["add_mul"]
        self.recs.append([t, P["name_prefix"] + str(t), 0])
        for r in self.recs:
            r[2] = self.score(r)
        self.recs = [r for r in self.recs if r[2] < P["keep_lim"] and not (P["drop7"] and r[0] % 7 == 0)]
        self.log.append(P["label"] + "0")
        k2, a2 = self.scorer
        self.scored += t * k2 + a2
        self.acc = self.acc + t * P["fold_mul"] + self.worker_add
    def tick(self):
        self.t += 1
        if self.inner_add is not None:
            self.count += self.inner_add
        self.step()
        return self.show()
    def show(self):
        table = ",".join(sorted(f"{k}={v}" for k, v in self.table.items()))
        recs = ",".join(f"{r[0]}:{r[1]}:{r[2]}" for r in self.recs)
        return f"count={self.count} table={table} recs={recs} log={''.join(self.log)} scored={self.scored} acc={self.acc}"

# The script: ("edit", {param: value} or raw, expect) / "tick" / "inner" / "end" / "renew"
def edit(expect, **ch):
    return ("edit", ch, expect)

LAYOUT = ("raw", "type Rec = { id: Int, name: String, score: Int, tag: String }\n\n"
          "fn name_of(t: Int) -> String {\n  \"r\" + to_string(t)\n}\n", ["Rec", "name_of"])
STATE = ("raw", "/// Everything the program holds between ticks.\ntype State = {\n  count: Int,\n  table: Dict[String, Int],\n  recs: List[Rec],\n  log: List[String],\n  scored: Int,\n  acc: Int,\n  extra: Int,\n}\n", ["State"])
SIG = ("raw", "fn bump(n: Int) -> Int {\n  n\n}\n\nfn step(mut s: State, t: Int, scorer: (Int) -> Int) {\n" +
       BASEDEFS["step"].split("{\n", 1)[1].replace("bump()", "bump(1)"), ["bump", "step"])
BROKEN = ("raw", "fn bump() -> Int {\n  nothing_here\n}\n", ["bump"])
MAIN = ("raw", BASEDEFS["main"].replace('mut term.print("renewed")', 'mut term.print("renewed!")'), ["main"])
CEX = ("raw", "/// Small numbers.\ntype Small = Int where it >= 0 && it < 10\n\n"
       "fn bump() -> Int {\n  small(7)\n}\n\n/// Twice `x`, said to stay small.\nfn small(x: Small) -> Small {\n  x * 2\n}\n", ["bump"], ["small", "Small"])
NEWTYPE_FIELD_REC = ("raw", "fn keep(r: Rec) -> Bool {\n  r.score < r.id + 100\n}\n", ["keep"])

T = ["tick"]
SCRIPT = (T * 3 +
    [edit("swap", bump=2)] + T * 2 +
    [edit("swap", key_mod=4)] + T * 2 +
    [edit("swap", key_prefix="key")] + T +
    [edit("swap", add_mul=3)] + T * 2 +
    [edit("swap", name_prefix="rec")] + T * 2 +
    [edit("swap", label="b")] + T * 2 +
    [edit("swap", score_add=2)] + T * 2 +
    [edit("swap", keep_lim=9)] + T * 3 +
    [edit("swap", score="grow", grow=1)] + T * 2 +           # a helper added
    [edit("swap", grow=3)] + T * 2 +                         # the helper only
    [edit("refused", **{"raw": LAYOUT})] + T * 2 +           # a record's layout
    [edit("refused", **{"raw": STATE})] + T +                # the state's layout
    [edit("refused", **{"raw": SIG})] + T +                  # a signature
    [edit("refused", **{"raw": BROKEN})] + T +               # doesn't check
    [edit("refused", **{"raw": MAIN})] + T +                 # main
    [edit("refused", **{"raw": CEX})] + T +                  # a counterexample
    [edit("swap", scorer_add=5)] + T * 2 +                   # the old closure stays
    ["renew"] + T * 2 +                                      # a new one is made
    [edit("swap", fold_mul=2)] + T * 2 +                     # the worker's helper
    [edit("swap", worker_add=7)] + T * 2 +                   # the worker's frame: stays
    ["inner"] + T * 2 +
    [edit("swap", inner_add=300)] + T * 2 +                  # the running frame: stays
    [edit("swap", bump=5)] + T * 2 +                         # called from that frame: changes
    ["end", "inner"] + T * 2 + ["end"] +                     # a new frame: the new code
    [edit("swap", label="cc", name_prefix="z")] + T * 2 +    # two at once
    [edit("swap", score="plain", score_add=1)] + T * 2 +     # back to the first code
    [edit("swap", key_prefix="k", key_mod=3)] + T * 2 +
    [edit("swap", bump=1, add_mul=1, label="a")] + T * 3 +
    [edit("swap", keep_lim=4)] + T * 3 +
    [edit("swap", drop7=True)] + T * 8 +                     # a lambda inside a function
    [edit("swap", drop7=False, keep_lim=6)] + T * 3)

def run():
    P = dict(P0) if MODE != "final" else eval(open("final_params.txt").read())
    with open("st.st", "w") as f:
        f.write(program(P))
    if os.path.exists("st.strata"):
        os.remove("st.strata")
    subprocess.run([S, "--file", "st.strata", "edit", "--as-given"], stdin=open("st.st"), capture_output=True)
    args = [S, "--file", "st.strata", "run"] + (["--live"] if MODE == "live" else [])
    err = open("err.txt", "w")
    p = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, text=True, bufsize=1)
    m = Model(P)
    n_edit = 0
    ok = bad = 0
    pending_rm = []
    restore = []         # definitions a refused edit left changed in the file
    lat = []
    def send(cmd):
        p.stdin.write(cmd + "\n"); p.stdin.flush()
    def check(line, want, what):
        nonlocal ok, bad
        if line == want:
            ok += 1
        else:
            bad += 1
            print(f"MISMATCH after {what}:\n  got  {line}\n  want {want}")
    for i, s in enumerate(SCRIPT):
        if s == "tick":
            send("tick")
            check(p.stdout.readline().rstrip("\n"), m.tick(), f"step {i} tick t={m.t + 1}")
        elif s == "inner":
            send("inner"); m.inner_add = m.P["inner_add"]
        elif s == "end":
            send("end"); m.inner_add = None
            check(p.stdout.readline().rstrip("\n"), "ended", f"step {i} end")
        elif s == "renew":
            send("renew"); m.scorer = (2, m.P["scorer_add"])
            check(p.stdout.readline().rstrip("\n"), "renewed", f"step {i} renew")
        else:
            _, ch, expect = s
            if MODE != "live":
                continue
            n_edit += 1
            newP = dict(m.P)
            rm = []
            if "raw" in ch:
                raw = ch["raw"]
                src = raw[1]
                rm = raw[3] if len(raw) > 3 else []
                touched = raw[2]
            else:
                newP.update(ch)
                names = sorted({d for k in ch for d in PARAM_DEFS[k]})
                if newP["score"] == "plain":
                    names = [d for d in names if d != "grow"]
                src = "\n".join(text(d, newP) for d in names)
                touched = names
            # what an earlier refused edit left in the files is put back
            src = "\n".join(text(d, m.P) for d in restore if d not in touched) + "\n" + src
            rm_args = []
            for d in pending_rm:
                rm_args += ["--rm", d]
            if "raw" not in ch and m.P["score"] != "plain" and newP["score"] == "plain":
                rm_args += ["--rm", "grow"]
            t0 = time.time()
            r = subprocess.run([S, "--file", "st.strata", "edit", "--as-given"] + rm_args, input=src, text=True, capture_output=True)
            note = None
            while time.time() - t0 < 60:
                txt = open("err.txt").read()
                mm = re.search(rf"note: live edit {n_edit}[: ].*", txt)
                if mm:
                    note = mm.group(0); break
                time.sleep(0.02)
            lat.append(time.time() - t0)
            got = "swap" if note and "swapped in" in note else "refused" if note else "nothing"
            print(f"edit {n_edit} ({', '.join(touched)}): {got}: {note}")
            if got != expect:
                bad += 1
                print(f"UNEXPECTED: wanted {expect}")
            if got == "swap":
                m.P = newP
                restore = []
                pending_rm = []
            else:
                restore = list(dict.fromkeys(restore + touched))
                pending_rm = list(dict.fromkeys(pending_rm + rm))
    send("quit")
    p.wait(timeout=30)
    print(f"{MODE}: {ok} lines as the model says, {bad} wrong, exit {p.returncode}")
    if lat:
        lat.sort()
        print(f"edit to note: median {lat[len(lat)//2]*1000:.0f} ms, max {lat[-1]*1000:.0f} ms")
    return m

final = run()
if MODE == "live":
    with open("final_params.txt", "w") as f:
        f.write(repr(final.P))
