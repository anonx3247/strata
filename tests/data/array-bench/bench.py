# The NumPy side of bench.st: `python3 bench.py` (NumPy's matmul is BLAS:
# Accelerate on macOS). Best of 5 runs, in microseconds.
import time
import numpy as np

def best(f, runs=5):
    t = []
    for _ in range(runs):
        a = time.perf_counter()
        f()
        t.append(time.perf_counter() - a)
    return min(t) * 1e6

for dt in (np.float64, np.int64, np.int32):
    x = np.arange(10_000_000).astype(dt)
    a = (np.arange(65536) % 5).astype(dt).reshape(256, 256)
    b = (np.arange(65536) % 3).astype(dt).reshape(256, 256)
    y = np.arange(10_000_000).astype(dt)
    print(f"{np.dtype(dt).name}: sum of 10M {best(lambda: x.sum()):.0f} us, "
          f"add of 10M {best(lambda: x + y):.0f} us, "
          f"matmul 256 {best(lambda: a @ b):.0f} us")
