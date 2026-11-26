# drive.py STRATA PROGRAM EDIT...: runs PROGRAM (a .st file) with `strata
# run --live`, two ticks, then each EDIT followed by three ticks (each edit
# once the run said what it did with it). Prints the program's lines, then
# its notes with the latencies as N.
import subprocess, sys, time, os, re

S, program, edits = sys.argv[1], sys.argv[2], sys.argv[3:]
if os.path.exists("demo.strata"):
    os.remove("demo.strata")
subprocess.run([S, "--file", "demo.strata", "edit"], stdin=open(program), capture_output=True)
p = subprocess.Popen([S, "--file", "demo.strata", "run", "--live"], stdin=subprocess.PIPE,
                     stdout=subprocess.PIPE, stderr=open("err.txt", "w"), text=True, bufsize=1)

def tick():
    try:
        p.stdin.write("tick\n")
        p.stdin.flush()
    except BrokenPipeError:
        return
    line = p.stdout.readline()
    if line:
        print(line.rstrip("\n"))

tick()
tick()
for n, e in enumerate(edits, 1):
    subprocess.run([S, "--file", "demo.strata", "edit"], stdin=open(e), capture_output=True)
    t0 = time.time()
    while f"live edit {n}" not in open("err.txt").read() and time.time() - t0 < 60:
        time.sleep(0.05)
    print("--", e)
    tick()
    tick()
    tick()
try:
    p.stdin.write("quit\n")
    p.stdin.flush()
except BrokenPipeError:
    pass
p.wait(timeout=30)
print(f"exit {p.returncode}")
for line in open("err.txt").read().splitlines():
    if not line.startswith("warning:"):
        print(re.sub(r"\d+ ms after", "N ms after", line))
