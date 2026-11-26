// memcurve OUT.csv CMD...: runs CMD and samples the memory of it and its
// children every 10 ms (macOS `proc_pid_rusage`: resident size and
// physical footprint, the number Activity Monitor shows). Writes
// `ms,rss_mb,footprint_mb` lines to OUT.csv; prints on stderr the peaks,
// when they came, the run's length and a 60-column plot of the footprint.
// CMD's output goes where it would; the exit code is CMD's. A sample costs
// a few microseconds a process. Built by tools/memprof.sh.
#include <libproc.h>
#include <signal.h>
#include <spawn.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

static double now_ms(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e3 + t.tv_nsec / 1e6;
}

// The resident and footprint bytes of `pid` and its descendants.
static void sample(pid_t pid, double *rss, double *fp, int depth) {
  struct rusage_info_v2 ri;
  if (proc_pid_rusage(pid, RUSAGE_INFO_V2, (rusage_info_t *)&ri) == 0) {
    *rss += (double)ri.ri_resident_size;
    *fp += (double)ri.ri_phys_footprint;
  }
  if (depth > 3) return;
  pid_t kids[256];
  int n = proc_listchildpids(pid, kids, sizeof kids);
  for (int i = 0; i < n && i < 256; i++) sample(kids[i], rss, fp, depth + 1);
}

int main(int argc, char **argv) {
  if (argc < 3) {
    fprintf(stderr, "usage: memcurve OUT.csv CMD...\n");
    return 2;
  }
  FILE *out = fopen(argv[1], "w");
  if (!out) return 2;
  pid_t pid;
  if (posix_spawnp(&pid, argv[2], NULL, NULL, argv + 2, environ) != 0) {
    perror("memcurve");
    return 127;
  }
  double t0 = now_ms(), peak_rss = 0, peak_fp = 0, at_rss = 0, at_fp = 0, t = 0;
  static double ts[1 << 20], fps[1 << 20];
  int n = 0, status = 0;
  fprintf(out, "ms,rss_mb,footprint_mb\n");
  for (;;) {
    pid_t r = waitpid(pid, &status, WNOHANG);
    if (r == pid) break;
    double rss = 0, fp = 0;
    sample(pid, &rss, &fp, 0);
    t = now_ms() - t0;
    rss /= 1048576.0;
    fp /= 1048576.0;
    fprintf(out, "%.0f,%.1f,%.1f\n", t, rss, fp);
    if (rss > peak_rss) peak_rss = rss, at_rss = t;
    if (fp > peak_fp) peak_fp = fp, at_fp = t;
    if (n < (1 << 20)) ts[n] = t, fps[n] = fp, n++;
    usleep(10000);
  }
  fclose(out);
  fprintf(stderr, "peak rss %.1f MB at %.0f ms, footprint %.1f MB at %.0f ms, of %.0f ms (%d samples)\n", peak_rss, at_rss,
          peak_fp, at_fp, t, n);
  if (n > 0 && peak_fp > 0) {
    int cols = 60, rows = 8;
    double h[60];
    for (int c = 0; c < cols; c++) {
      // the highest sample in the column
      int a = c * n / cols, b = (c + 1) * n / cols;
      double m = 0;
      for (int i = a; i < b || i == a; i++) if (i < n && fps[i] > m) m = fps[i];
      h[c] = m / peak_fp * rows;
    }
    for (int y = rows; y >= 1; y--) {
      fprintf(stderr, "%7.0f |", peak_fp * y / rows);
      for (int c = 0; c < cols; c++) fputc(h[c] >= y - 0.5 ? '#' : ' ', stderr);
      fputc('\n', stderr);
    }
    fprintf(stderr, "        +------------------------------------------------------------\n         0 ms%49.0f ms\n", t);
  }
  return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}
