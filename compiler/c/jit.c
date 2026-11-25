// The JIT the compiler runs programs with: LLVM's LLJIT through its C API.
// The IR is codegen's. What it declares is found in the libraries loaded
// first (`jit_load`), then in this process, the compiler's own executable,
// which runtime.c is linked into (the runtime's `strata_*` functions).

#include <llvm-c/Core.h>
#include <llvm-c/Error.h>
#include <llvm-c/IRReader.h>
#include <llvm-c/LLJIT.h>
#include <llvm-c/Orc.h>
#include <llvm-c/Support.h>
#include <llvm-c/Target.h>
#include <llvm-c/TargetMachine.h>
#include <llvm-c/Transforms/PassBuilder.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <spawn.h>
#include <stdatomic.h>
#include <stdint.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// The libraries `jit_load` loaded (std's precompiled code, a package's C),
// in order. The JIT finds a symbol in them before it looks in this process.
// The compiler's executable is a Strata program too, with its own copies of
// std's functions, built from its own std, and those must not stand in for
// the library's.
static char *loaded_libs[64];
static int loaded_count;

// Where a JIT looks for what its module declares: each loaded library, then
// this process (the runtime's `strata_*` functions). An error, or NULL.
static LLVMErrorRef add_generators(LLVMOrcLLJITRef jit) {
  LLVMOrcJITDylibRef main = LLVMOrcLLJITGetMainJITDylib(jit);
  char prefix = LLVMOrcLLJITGetGlobalPrefix(jit);
  LLVMOrcDefinitionGeneratorRef gen;
  for (int i = 0; i < loaded_count; i++) {
    LLVMErrorRef e = LLVMOrcCreateDynamicLibrarySearchGeneratorForPath(&gen, loaded_libs[i], prefix, NULL, NULL);
    if (e) return e;
    LLVMOrcJITDylibAddGenerator(main, gen);
  }
  LLVMErrorRef e = LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess(&gen, prefix, NULL, NULL);
  if (e) return e;
  LLVMOrcJITDylibAddGenerator(main, gen);
  return NULL;
}

// How programs and tests are compiled (`jit_set_opt`):
// -1: LLVM's JIT default, its code generator at -O2 and no IR passes, for
//     `verify`'s fuzzing and compile-time code.
//  0: unoptimized, for `strata run` and `strata test`: several times faster
//     to compile.
//  2: as clang -O2, IR passes included, for `--release`.
static int jit_opt = -1;

int32_t jit_set_opt(int32_t level) {
  jit_opt = level;
  return 0;
}

// `--timings` (c/driver.c): where a phase begins and ends, and the
// phases' times written out.
int32_t timings_begin(const char *name);
int32_t timings_end(const char *name);
int32_t timings_report(void);

// Whether code is compiled for profiling (`--profile`, or STRATA_PROFILE
// set and not `0`). Every function then keeps its frame pointer
// ("frame-pointer"="all"), so that a sampling profiler (macOS's `sample`)
// walks through Strata's frames. Off by default.
static int profiling = -1;

int32_t toolchain_profiling(void) {
  if (profiling < 0) {
    const char *p = getenv("STRATA_PROFILE");
    profiling = p && *p && strcmp(p, "0") != 0;
  }
  return profiling;
}

// `--profile`: profiling on, for this process and the ones it starts.
int32_t toolchain_set_profile(int32_t on) {
  profiling = on != 0;
  if (on) setenv("STRATA_PROFILE", "1", 1);
  return 0;
}

// When profiling, every function the module defines keeps its frame
// pointer.
void toolchain_frame_pointers(LLVMModuleRef mod) {
  if (!toolchain_profiling()) return;
  LLVMContextRef ctx = LLVMGetModuleContext(mod);
  for (LLVMValueRef f = LLVMGetFirstFunction(mod); f; f = LLVMGetNextFunction(f)) {
    if (LLVMIsDeclaration(f)) continue;
    LLVMRemoveStringAttributeAtIndex(f, LLVMAttributeFunctionIndex, "frame-pointer", 13);
    LLVMAddAttributeAtIndex(f, LLVMAttributeFunctionIndex,
                               LLVMCreateStringAttribute(ctx, "frame-pointer", 13, "all", 3));
  }
}

// Instruction selection by SelectionDAG, and FastISel at -O0, instead of
// the GlobalISel LLVM 23 turns on for every AArch64 target machine. At -O0
// it takes half the time for compiler/'s code (4.5 s -> 2.5 s), with code
// as good.
//
// It is set on each target machine (c/toolchain.c's objects too), never as
// a process-wide option. `-global-isel=0` holds only for the first module a
// process compiles, and GlobalISel leaves state behind: an object compiled
// after the JIT (compile-time values, for the host's CPU) would differ from
// the same IR compiled alone (an `optnone` function aligned to 64 bytes
// instead of 4). A build's bytes would then depend on whether its
// compile-time values were cached.
void toolchain_selection(LLVMTargetMachineRef tm) { LLVMSetTargetMachineGlobalISel(tm, 0); }

// A JIT's builder for `jit_opt`: the host's CPU and features, at its level
// (-O2's machine code when no level is set, as LLJIT's own default).
static LLVMOrcLLJITBuilderRef jit_builder(void) {
  LLVMOrcLLJITBuilderRef b = LLVMOrcCreateLLJITBuilder();
  char *triple = LLVMGetDefaultTargetTriple();
  LLVMTargetRef target;
  char *msg = NULL;
  if (LLVMGetTargetFromTriple(triple, &target, &msg)) {
    LLVMDisposeMessage(msg);
    LLVMDisposeMessage(triple);
    return b;
  }
  char *cpu = LLVMGetHostCPUName();
  char *features = LLVMGetHostCPUFeatures();
  LLVMTargetMachineRef tm = LLVMCreateTargetMachine(target, triple, cpu, features,
      jit_opt == 0 ? LLVMCodeGenLevelNone : LLVMCodeGenLevelDefault, LLVMRelocDefault, LLVMCodeModelJITDefault);
  toolchain_selection(tm);
  LLVMDisposeMessage(cpu);
  LLVMDisposeMessage(features);
  LLVMDisposeMessage(triple);
  // (the builder owns the target machine from here)
  LLVMOrcLLJITBuilderSetJITTargetMachineBuilder(b, LLVMOrcJITTargetMachineBuilderCreateFromTargetMachine(tm));
  return b;
}

// The module optimized as clang -O2 would, with `--release` (jit_opt 2).
static void optimized(LLVMModuleRef mod) {
  if (jit_opt < 2) return;
  char *triple = LLVMGetDefaultTargetTriple();
  LLVMTargetRef target;
  char *msg = NULL;
  if (!LLVMGetTargetFromTriple(triple, &target, &msg)) {
    char *cpu = LLVMGetHostCPUName();
    char *features = LLVMGetHostCPUFeatures();
    LLVMTargetMachineRef tm = LLVMCreateTargetMachine(target, triple, cpu, features,
        LLVMCodeGenLevelDefault, LLVMRelocDefault, LLVMCodeModelJITDefault);
    LLVMPassBuilderOptionsRef o = LLVMCreatePassBuilderOptions();
    LLVMErrorRef e = LLVMRunPasses(mod, "default<O2>", tm, o);
    if (e) LLVMConsumeError(e);
    LLVMDisposePassBuilderOptions(o);
    LLVMDisposeTargetMachine(tm);
    LLVMDisposeMessage(cpu);
    LLVMDisposeMessage(features);
  } else {
    LLVMDisposeMessage(msg);
  }
  LLVMDisposeMessage(triple);
}

// The IR text as a module, in a context of its own (c/ir.c): 0, else 1
// and why.
int32_t toolchain_module(const char *ir, LLVMContextRef *ctx, LLVMModuleRef *mod, char **msg);
// (the same, untimed: callable from another thread)
int32_t ir_module(const char *ir, size_t len, LLVMContextRef *ctx, LLVMModuleRef *mod, char **msg);

// Whether `strata run` compiles the program swappable (`swappable_module`):
// a verifier runs alongside it (`jit_alongside`).
static int swappable;
static void swappable_module(LLVMModuleRef mod);
static LLVMOrcLLJITRef run_jit;

static int failed(LLVMErrorRef e, const char *what) {
  char *m = LLVMGetErrorMessage(e);
  fprintf(stderr, "error: the JIT %s: %s\n", what, m);
  LLVMDisposeErrorMessage(m);
  return 70;
}

// The IR compiled by a JIT of its own, the runtime (`strata_*`) found in
// this process: NULL when it can't be (after saying why).
static LLVMOrcLLJITRef compiled(const char *ir) {
  // (the machine's own target, whichever it is)
  if (LLVMInitializeNativeTarget() || LLVMInitializeNativeAsmPrinter()) {
    fprintf(stderr, "error: the JIT has no target for this machine\n");
    return NULL;
  }
  LLVMContextRef ctx;
  LLVMModuleRef mod;
  char *msg = NULL;
  if (toolchain_module(ir, &ctx, &mod, &msg)) {
    fprintf(stderr, "error: LLVM rejected the generated IR: %s\n", msg ? msg : "");
    LLVMDisposeMessage(msg);
    return NULL;
  }
  LLVMOrcLLJITRef jit;
  toolchain_frame_pointers(mod);
  if (swappable) swappable_module(mod);
  timings_begin("LLVM: optimize");
  optimized(mod);
  timings_end("LLVM: optimize");
  LLVMErrorRef e = LLVMOrcCreateLLJIT(&jit, jit_builder());
  if (e) return failed(e, "can't start"), NULL;
  // the loaded libraries, then the runtime: this process's own symbols
  e = add_generators(jit);
  if (e) return failed(e, "can't see the runtime"), NULL;
  LLVMOrcThreadSafeContextRef tsc = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(ctx);
  LLVMOrcThreadSafeModuleRef tsm = LLVMOrcCreateNewThreadSafeModule(mod, tsc);
  e = LLVMOrcLLJITAddLLVMIRModule(jit, LLVMOrcLLJITGetMainJITDylib(jit), tsm);
  if (e) return failed(e, "can't add the program"), NULL;
  run_jit = jit;
  return jit;
}

static void alongside_begin(void);

// Runs the IR's `main` with these arguments (the program's name first, each
// after a newline). Gives its exit code, or 70 when the JIT can't run it.
int32_t jit_run_main(const char *ir, const char *args) {
  LLVMOrcLLJITRef jit = compiled(ir);
  if (!jit) return 70;
  LLVMOrcExecutorAddress addr;
  // (the JIT compiles the program when it is looked up)
  timings_begin("JIT: machine code");
  LLVMErrorRef e = LLVMOrcLLJITLookup(jit, &addr, "main");
  timings_end("JIT: machine code");
  if (e) return failed(e, "has no main");
  // (the program's own time isn't the compiler's: the times are written
  // before it runs)
  timings_report();
  // (a verifier running alongside: its proven code swapped in when it ends)
  alongside_begin();
  int (*entry)(int, char **) = (int (*)(int, char **))(uintptr_t)addr;
  // the program's name and arguments, separated by newlines
  char *copy = strdup(args);
  char *argv[256];
  int argc = 0;
  for (char *p = copy, *at = copy; argc < 255; p++) {
    if (*p == '\n' || *p == 0) {
      int end = *p == 0;
      *p = 0;
      argv[argc++] = at;
      at = p + 1;
      if (end) break;
    }
  }
  argv[argc] = NULL;
  int code = entry(argc, argv);
  // (the program's exit code is the process's: nothing runs after it)
  fflush(stdout);
  fflush(stderr);
  exit(code);
}

// ---- one function called (`strata run f ARGS`) ----
//
// compiler/codegen's `strata.call.0` (`codegen --call`) calls the function
// on its arguments, written as code, and hands over the result's `debug`
// text (`strata_ct_keep`): the result as the code that writes it. It runs
// as a test runs (`strata_test`), so a fault comes back instead of ending
// the process.

typedef struct {
  int64_t len;
  char bytes[];
} Str;

// (the runtime's, in this process: looked up when called, since this file
// is also linked where there is no runtime)
static void *runtime_fn(const char *name) { return dlsym(RTLD_DEFAULT, name); }

// Runs `strata.call.0`. Gives:
// 0: the call ended. What it printed, then its result on a line of its own.
// 1: a fault, on stderr, with its call stack below the probe `probe`.
// 2: invalid input, the refinement of an argument of `fun` broken on entry.
//    The message is on stderr after `error: invalid input: `.
// 70: the JIT can't run it.
static int32_t run_call(const char *ir, const char *probe, const char *fun) {
  LLVMOrcLLJITRef jit = compiled(ir);
  if (!jit) return 70;
  LLVMOrcExecutorAddress addr;
  timings_begin("JIT: machine code");
  LLVMErrorRef e = LLVMOrcLLJITLookup(jit, &addr, "strata.call.0");
  timings_end("JIT: machine code");
  if (e) return failed(e, "has no call");
  int32_t (*test)(void (*)(void)) = runtime_fn("strata_test");
  int64_t (*kept)(void) = runtime_fn("strata_ct_kept");
  const char *(*kind)(void) = runtime_fn("strata_test_kind");
  Str *(*message)(void) = runtime_fn("strata_test_message");
  const char *(*stack)(void) = runtime_fn("strata_test_stack");
  const char *(*printed)(void) = runtime_fn("strata_test_printed");
  if (!test || !kept || !kind || !message || !stack || !printed) {
    fprintf(stderr, "error: the JIT can't find the runtime\n");
    return 70;
  }
  int32_t r = test((void (*)(void))(uintptr_t)addr);
  fputs(printed(), stdout);
  if (r == 0) {
    Str *v = (Str *)(uintptr_t)kept();
    printf("%.*s\n", (int)v->len, v->bytes);
    fflush(stdout);
    return 0;
  }
  fflush(stdout);
  Str *m = message();
  // the call stack without the probe
  const char *all = stack();
  char *frames = malloc(strlen(all) + 1);
  frames[0] = 0;
  int count = 0;
  for (const char *at = all; *at;) {
    const char *end = strstr(at, " -> ");
    size_t n = end ? (size_t)(end - at) : strlen(at);
    if (n && !(n == strlen(probe) && !strncmp(at, probe, n))) {
      if (count++) strcat(frames, " -> ");
      strncat(frames, at, n);
    }
    at += n + (end ? 4 : 0);
  }
  // `argument `P` of `F`: ...`, found on entry to `F`
  char head[1024];
  snprintf(head, sizeof head, "` of `%s`: ", fun);
  if (!strcmp(kind(), "Refinement") && count <= 1 && m->len > 10 && !strncmp(m->bytes, "argument `", 10)) {
    const char *rest = memmem(m->bytes + 10, m->len - 10, head, strlen(head));
    if (rest) {
      fprintf(stderr, "error: invalid input: %.*s: %.*s\n", (int)(rest - m->bytes - 10), m->bytes + 10, (int)(m->len - (rest - m->bytes) - strlen(head)), rest + strlen(head));
      return 2;
    }
  }
  fprintf(stderr, "fault (%s): %.*s\n", kind(), (int)m->len, m->bytes);
  if (count) fprintf(stderr, "    stack: %s\n", frames);
  return 1;
}

// (the exit code is the process's, as `jit_run_main`'s)
int32_t jit_run_call(const char *ir, const char *probe, const char *fun) {
  int32_t code = run_call(ir, probe, fun);
  fflush(stdout);
  fflush(stderr);
  exit(code);
}

// Loads a shared library into this process for the JIT to find its symbols
// (std's precompiled code): 0 when loaded, else 1 after saying why.
int32_t jit_load(const char *path) {
  if (dlopen(path, RTLD_NOW | RTLD_GLOBAL)) {
    if (loaded_count < 64) loaded_libs[loaded_count++] = strdup(path);
    return 0;
  }
  fprintf(stderr, "note: can't load %s: %s\n", path, dlerror());
  return 1;
}

// ---- fuzzing (the compiler's own `verify`) ----
//
// The fuzz drivers compiler/codegen makes (`codegen --fuzz-text`, one
// `strata.fuzz.N` per function checked) compiled once, then run case by case
// in this process: `strata_fuzz` (runtime.c) runs one as a test, seeded and
// within a step budget, and the fault that ended it, if any, and the case's
// arguments (as code) are read back.

// Each module compiled (a program's fuzz drivers, a batch of probes), kept
// while the compiler runs.
static LLVMOrcLLJITRef fuzz_jits[4096];
static int fuzz_count;
static char *fuzz_error;

// Compiles the drivers' IR: its handle (0 and up), else -1, and
// `jit_fuzz_error` says why.
int32_t jit_fuzz_load(const char *ir) {
  free(fuzz_error);
  fuzz_error = NULL;
  if (LLVMInitializeNativeTarget() || LLVMInitializeNativeAsmPrinter()) {
    fuzz_error = strdup("the JIT has no target for this machine");
    return -1;
  }
  LLVMContextRef ctx;
  LLVMModuleRef mod;
  char *msg = NULL;
  if (toolchain_module(ir, &ctx, &mod, &msg)) {
    fuzz_error = strdup(msg ? msg : "LLVM rejected the generated IR");
    LLVMDisposeMessage(msg);
    return -1;
  }
  if (fuzz_count == 4096) {
    fuzz_error = strdup("too many modules compiled");
    return -1;
  }
  LLVMOrcLLJITRef fuzz_jit;
  optimized(mod);
  LLVMErrorRef e = LLVMOrcCreateLLJIT(&fuzz_jit, jit_builder());
  if (!e) {
    e = add_generators(fuzz_jit);
    if (!e) {
      LLVMOrcThreadSafeContextRef tsc = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(ctx);
      LLVMOrcThreadSafeModuleRef tsm = LLVMOrcCreateNewThreadSafeModule(mod, tsc);
      e = LLVMOrcLLJITAddLLVMIRModule(fuzz_jit, LLVMOrcLLJITGetMainJITDylib(fuzz_jit), tsm);
    }
  }
  if (e) {
    char *m = LLVMGetErrorMessage(e);
    fuzz_error = strdup(m);
    LLVMDisposeErrorMessage(m);
    return -1;
  }
  fuzz_jits[fuzz_count] = fuzz_jit;
  return fuzz_count++;
}

const char *jit_fuzz_error(void) { return fuzz_error ? fuzz_error : ""; }

// One case of driver `i` of module `m`: 0 when it passed, 1 when a fault
// ended it, -1 when there is no such driver.
int32_t jit_fuzz_case(int32_t m, int64_t i, int64_t seed, int64_t budget) {
  if (m < 0 || m >= fuzz_count) return -1;
  LLVMOrcLLJITRef fuzz_jit = fuzz_jits[m];
  char name[64];
  snprintf(name, sizeof name, "strata.fuzz.%lld", (long long)i);
  LLVMOrcExecutorAddress addr;
  LLVMErrorRef e = LLVMOrcLLJITLookup(fuzz_jit, &addr, name);
  if (e) {
    LLVMConsumeError(e);
    return -1;
  }
  int32_t (*fuzz)(void (*)(void), int64_t, int64_t) = runtime_fn("strata_fuzz");
  if (!fuzz) return -1;
  return fuzz((void (*)(void))(uintptr_t)addr, seed, budget) != 0;
}

static const char *runtime_text(const char *name) {
  const char *(*f)(void) = runtime_fn(name);
  return f ? f() : "";
}

const char *jit_fault_kind(void) { return runtime_text("strata_test_kind"); }

static char *fault_text;
const char *jit_fault_message(void) {
  Str *(*message)(void) = runtime_fn("strata_test_message");
  Str *m = message ? message() : NULL;
  free(fault_text);
  fault_text = m ? strndup(m->bytes, (size_t)m->len) : strdup("");
  return fault_text;
}

const char *jit_fault_stack(void) { return runtime_text("strata_test_stack"); }

const char *jit_fuzz_args(void) { return runtime_text("strata_fuzz_args"); }

// ---- tests (`strata test`) ----
//
// The tests compiler/codegen makes (`codegen --tests`, one `strata.test.N`
// per test block), loaded by `jit_fuzz_load`, each run as `strata test`
// runs it (`strata_test`): a fault ends the test, not the process; what it
// printed is kept (`jit_test_printed`), and the fault read with
// `jit_fault_kind`, `jit_fault_message` and `jit_fault_stack`.

// Runs test `i` of module `m`: 0 when it passed, 1 when a fault ended it,
// -1 when there is no such test.
int32_t jit_test_case(int32_t m, int64_t i) {
  if (m < 0 || m >= fuzz_count) return -1;
  char name[64];
  snprintf(name, sizeof name, "strata.test.%lld", (long long)i);
  LLVMOrcExecutorAddress addr;
  LLVMErrorRef e = LLVMOrcLLJITLookup(fuzz_jits[m], &addr, name);
  if (e) {
    LLVMConsumeError(e);
    return -1;
  }
  int32_t (*test)(void (*)(void)) = runtime_fn("strata_test");
  if (!test) return -1;
  int32_t r = test((void (*)(void))(uintptr_t)addr) != 0;
  fflush(stdout);
  return r;
}

// What the last test printed.
const char *jit_test_printed(void) { return runtime_text("strata_test_printed"); }

// ---- compile-time values (`comptime e`, compiler/derive) ----
//
// A module of calls (`codegen` with calls: `strata.call.N`, each handing its
// value over with `strata_ct_keep`), loaded by `jit_fuzz_load`, run call by
// call as a fuzz case runs: within a step budget, a fault caught. The value
// stays in this process for the compiler to read (core.values' `Raw`).

// Calls `strata.call.i` of module `m`: 0 when it ended (its value's address
// in `jit_kept`), 1 when a fault ended it (`jit_fault_message`), -1 when
// there is no such call.
int32_t jit_call_case(int32_t m, int64_t i, int64_t budget) {
  if (m < 0 || m >= fuzz_count) return -1;
  char name[64];
  snprintf(name, sizeof name, "strata.call.%lld", (long long)i);
  LLVMOrcExecutorAddress addr;
  LLVMErrorRef e = LLVMOrcLLJITLookup(fuzz_jits[m], &addr, name);
  if (e) {
    LLVMConsumeError(e);
    return -1;
  }
  int32_t (*fuzz)(void (*)(void), int64_t, int64_t) = runtime_fn("strata_fuzz");
  if (!fuzz) return -1;
  return fuzz((void (*)(void))(uintptr_t)addr, 7, budget) != 0;
}

// The address of the value the last call handed over.
int64_t jit_kept(void) {
  int64_t (*kept)(void) = runtime_fn("strata_ct_kept");
  return kept ? kept() : 0;
}

// ---- `strata repl`'s values ----
//
// The values a REPL session keeps, in compiled memory: one 64-bit slot each
// (as a block keeps a field), the session's for its whole life. Each line's
// call (codegen's `session_entry`) reads its arguments from here and a
// `mut` one is assigned through its slot here, so what a line changes is
// kept even when a fault ends it.

#define REPL_WORDS 4096
static int64_t repl_words[REPL_WORDS];

// The slots, for the compiled code (looked up in this process).
int64_t *strata_repl_words(void) { return repl_words; }

int64_t jit_repl_word(int64_t i) { return i >= 0 && i < REPL_WORDS ? repl_words[i] : 0; }

int32_t jit_repl_set_word(int64_t i, int64_t w) {
  if (i < 0 || i >= REPL_WORDS) return 1;
  repl_words[i] = w;
  return 0;
}

// ---- verifying alongside a run, then swapping in the proven code ----
//
// `strata run` on a program whose verdicts aren't cached starts it at once,
// compiled with all its runtime checks, while a verifier (`strata
// verify-alongside`, a process of its own, `jit_alongside`) verifies it and
// compiles it again without the checks it proved. The program is compiled
// swappable (`swappable_module`): each of its own functions that can change
// (`strata:NAME`, `main` aside) is called through a stub that jumps to
// where its slot (`NAME.slot`) points, at first its checked body
// (`NAME.checked`). When the verifier ends, its module is added to the JIT
// (`swap_in`): each function whose code differs is compiled under a new
// name (`NAME.proven.N`) and its slot pointed at it, with one atomic store,
// so a call already running the checked body finishes in it and every call
// after (from any task) takes the proven one. Only runs with a verifier
// alongside are compiled this way: a run without one, and every `strata
// build` executable, calls its functions directly.

extern char **environ;

// What the main module defines (functions, globals), by name: what the
// verifier's module refers to rather than defines again.
typedef struct {
  char **names;
  int count, cap;
} Names;

static void names_add(Names *n, const char *s) {
  if (n->count == n->cap) {
    n->cap = n->cap ? n->cap * 2 : 256;
    n->names = realloc(n->names, sizeof(char *) * n->cap);
  }
  n->names[n->count++] = strdup(s);
}

static int names_cmp(const void *a, const void *b) { return strcmp(*(char *const *)a, *(char *const *)b); }

static void names_sort(Names *n) { qsort(n->names, n->count, sizeof(char *), names_cmp); }

static int names_index(const Names *n, const char *s) {
  char **at = n->count ? bsearch(&s, n->names, n->count, sizeof(char *), names_cmp) : NULL;
  return at ? (int)(at - n->names) : -1;
}

static Names defined;
// The swappable functions, sorted, and each one's code as first compiled
// (printed: a function the verifier's module has the same is left alone).
static Names swappable_names;
static char **swappable_code;

static pid_t verifier;
static char *alongside_dir;
static int alongside_notes;
static pthread_t watcher;
// (held while the proven code is swapped in: the process doesn't end in
// the middle of it, and nothing is swapped in once it is ending)
static pthread_mutex_t swap_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t swap_done = PTHREAD_COND_INITIALIZER;
static int ending, verified, swapped, swaps;
static char *verifier_said;

static int is_local(LLVMLinkage l) { return l == LLVMPrivateLinkage || l == LLVMInternalLinkage; }

static int starts(const char *s, const char *prefix) { return !strncmp(s, prefix, strlen(prefix)); }

static const char *name_of(LLVMValueRef v) {
  size_t n;
  return LLVMGetValueName2(v, &n);
}

// Whether `f` is one of the program's own functions that a proof can
// change: `strata:NAME` (not `main`, whose frame lasts the whole run),
// defined here, without attributes a stub's tail call would have to
// repeat.
static int can_swap(LLVMValueRef f) {
  if (LLVMIsDeclaration(f) || is_local(LLVMGetLinkage(f))) return 0;
  const char *name = name_of(f);
  if (!starts(name, "strata:") || !strcmp(name, "strata:main")) return 0;
  if (LLVMIsFunctionVarArg(LLVMGlobalGetValueType(f))) return 0;
  unsigned n = LLVMCountParams(f);
  if (LLVMGetAttributeCountAtIndex(f, LLVMAttributeReturnIndex)) return 0;
  for (unsigned i = 1; i <= n; i++)
    if (LLVMGetAttributeCountAtIndex(f, i)) return 0;
  return 1;
}

// `f` (named `name`) as a stub: its body renamed `NAME.checked`, a slot
// `NAME.slot` holding its address, and `NAME` loading the slot and jumping
// there (a tail call: the stub leaves no frame).
static void stub(LLVMModuleRef mod, LLVMValueRef f, const char *name) {
  LLVMContextRef ctx = LLVMGetModuleContext(mod);
  LLVMTypeRef ty = LLVMGlobalGetValueType(f);
  size_t len = strlen(name);
  char *other = malloc(len + 16);
  snprintf(other, len + 16, "%s.checked", name);
  LLVMSetValueName2(f, other, strlen(other));
  LLVMValueRef s = LLVMAddFunction(mod, name, ty);
  LLVMSetFunctionCallConv(s, LLVMGetFunctionCallConv(f));
  LLVMReplaceAllUsesWith(f, s);
  LLVMTypeRef ptr = LLVMPointerTypeInContext(ctx, 0);
  snprintf(other, len + 16, "%s.slot", name);
  LLVMValueRef slot = LLVMAddGlobal(mod, ptr, other);
  LLVMSetInitializer(slot, f);
  LLVMSetAlignment(slot, 8);
  free(other);
  LLVMBuilderRef b = LLVMCreateBuilderInContext(ctx);
  LLVMPositionBuilderAtEnd(b, LLVMAppendBasicBlockInContext(ctx, s, "entry"));
  // (atomic, so that a call reads either body's address whole; monotonic:
  // the new body was compiled and its memory made executable before its
  // address was stored, and an acquire wouldn't order fetching its
  // instructions anyway)
  LLVMValueRef at = LLVMBuildLoad2(b, ptr, slot, "at");
  LLVMSetOrdering(at, LLVMAtomicOrderingMonotonic);
  LLVMSetAlignment(at, 8);
  unsigned n = LLVMCountParams(s);
  LLVMValueRef *args = malloc(sizeof(LLVMValueRef) * (n ? n : 1));
  for (unsigned i = 0; i < n; i++) args[i] = LLVMGetParam(s, i);
  LLVMValueRef call = LLVMBuildCall2(b, ty, at, args, n, "");
  LLVMSetInstructionCallConv(call, LLVMGetFunctionCallConv(f));
  LLVMSetTailCallKind(call, LLVMTailCallKindMustTail);
  if (LLVMGetTypeKind(LLVMGetReturnType(ty)) == LLVMVoidTypeKind)
    LLVMBuildRetVoid(b);
  else
    LLVMBuildRet(b, call);
  LLVMDisposeBuilder(b);
  free(args);
}

// The program's module made swappable (before it is optimized): its own
// functions called through slots (`stub`), and every global it defines
// visible (`static`s, constants, tables), so that the verifier's module
// uses these rather than copies of its own.
static void swappable_module(LLVMModuleRef mod) {
  for (LLVMValueRef g = LLVMGetFirstGlobal(mod); g; g = LLVMGetNextGlobal(g)) {
    if (LLVMIsDeclaration(g) || starts(name_of(g), "llvm.")) continue;
    if (is_local(LLVMGetLinkage(g))) {
      LLVMSetLinkage(g, LLVMExternalLinkage);
      LLVMSetVisibility(g, LLVMHiddenVisibility);
    }
    names_add(&defined, name_of(g));
  }
  int count = 0;
  for (LLVMValueRef f = LLVMGetFirstFunction(mod); f; f = LLVMGetNextFunction(f)) {
    if (LLVMIsDeclaration(f) || is_local(LLVMGetLinkage(f))) continue;
    names_add(&defined, name_of(f));
    if (can_swap(f)) count++;
  }
  LLVMValueRef *fs = malloc(sizeof(LLVMValueRef) * (count ? count : 1));
  int k = 0;
  for (LLVMValueRef f = LLVMGetFirstFunction(mod); f; f = LLVMGetNextFunction(f))
    if (can_swap(f)) fs[k++] = f;
  for (int i = 0; i < count; i++) names_add(&swappable_names, name_of(fs[i]));
  names_sort(&swappable_names);
  swappable_code = calloc(count ? count : 1, sizeof(char *));
  for (int i = 0; i < count; i++) {
    char *name = strdup(name_of(fs[i]));
    swappable_code[names_index(&swappable_names, name)] = LLVMPrintValueToString(fs[i]);
    stub(mod, fs[i], name);
    free(name);
  }
  free(fs);
  names_sort(&defined);
}

// `v` (a function or a global of `mod`) as a declaration of the same name:
// the definition the main module has is the one used.
static void declared(LLVMModuleRef mod, LLVMValueRef v, int function) {
  char *name = strdup(name_of(v));
  LLVMSetValueName2(v, "", 0);
  LLVMTypeRef ty = LLVMGlobalGetValueType(v);
  LLVMValueRef d;
  if (function) {
    d = LLVMAddFunction(mod, name, ty);
    LLVMSetFunctionCallConv(d, LLVMGetFunctionCallConv(v));
  } else {
    d = LLVMAddGlobal(mod, ty, name);
    LLVMSetThreadLocal(d, LLVMIsThreadLocal(v));
    LLVMSetGlobalConstant(d, LLVMIsGlobalConstant(v));
  }
  LLVMReplaceAllUsesWith(v, d);
  if (function)
    LLVMDeleteFunction(v);
  else
    LLVMDeleteGlobal(v);
  free(name);
}

// The verifier's module (the program compiled without the checks it
// proved) added to the running program: each swappable function whose code
// changed compiled as `NAME.proven.N` and its slot pointed at it; what
// else it defines is the main module's (declared), or its own (local).
// How many functions were swapped; -1 when it couldn't be (said in `why`).
static int swap_in(const char *ir, size_t len, char *why, size_t why_len) {
  LLVMContextRef ctx;
  LLVMModuleRef mod;
  char *msg = NULL;
  if (ir_module(ir, len, &ctx, &mod, &msg)) {
    snprintf(why, why_len, "LLVM rejected it: %s", msg ? msg : "");
    LLVMDisposeMessage(msg);
    return -1;
  }
  int generation = ++swaps;
  // (each function's code read before any is renamed or declared)
  int count = 0;
  for (LLVMValueRef f = LLVMGetFirstFunction(mod); f; f = LLVMGetNextFunction(f)) count++;
  LLVMValueRef *fs = malloc(sizeof(LLVMValueRef) * (count ? count : 1));
  char **changed = calloc(count ? count : 1, sizeof(char *));
  int k = 0;
  for (LLVMValueRef f = LLVMGetFirstFunction(mod); f; f = LLVMGetNextFunction(f)) fs[k++] = f;
  for (int i = 0; i < count; i++) {
    LLVMValueRef f = fs[i];
    if (LLVMIsDeclaration(f) || is_local(LLVMGetLinkage(f))) continue;
    int at = names_index(&swappable_names, name_of(f));
    if (at >= 0) {
      char *code = LLVMPrintValueToString(f);
      if (strcmp(code, swappable_code[at])) changed[i] = strdup(name_of(f));      LLVMDisposeMessage(code);
    }
  }
  int n = 0;
  for (int i = 0; i < count; i++) {
    LLVMValueRef f = fs[i];
    if (LLVMIsDeclaration(f) || is_local(LLVMGetLinkage(f))) continue;
    if (changed[i]) {
      char name[1024];
      snprintf(name, sizeof name, "%s.proven.%d", changed[i], generation);
      LLVMSetValueName2(f, name, strlen(name));
      n++;
    } else if (names_index(&defined, name_of(f)) >= 0) {
      declared(mod, f, 1);
    } else {
      LLVMSetLinkage(f, LLVMInternalLinkage);
    }
  }
  free(fs);
  // the globals: the main module's, except its own new ones
  int gcount = 0;
  for (LLVMValueRef g = LLVMGetFirstGlobal(mod); g; g = LLVMGetNextGlobal(g)) gcount++;
  LLVMValueRef *gs = malloc(sizeof(LLVMValueRef) * (gcount ? gcount : 1));
  k = 0;
  for (LLVMValueRef g = LLVMGetFirstGlobal(mod); g; g = LLVMGetNextGlobal(g)) gs[k++] = g;
  for (int i = 0; i < gcount; i++) {
    LLVMValueRef g = gs[i];
    if (starts(name_of(g), "llvm.")) {
      // (constructors already ran)
      LLVMDeleteGlobal(g);
    } else if (LLVMIsDeclaration(g)) {
      continue;
    } else if (names_index(&defined, name_of(g)) >= 0) {
      declared(mod, g, 0);
    } else {
      LLVMSetLinkage(g, LLVMInternalLinkage);
    }
  }
  free(gs);
  if (n == 0) {
    for (int i = 0; i < count; i++) free(changed[i]);
    free(changed);
    LLVMDisposeModule(mod);
    LLVMContextDispose(ctx);
    return 0;
  }
  toolchain_frame_pointers(mod);
  optimized(mod);
  LLVMOrcThreadSafeContextRef tsc = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(ctx);
  LLVMOrcThreadSafeModuleRef tsm = LLVMOrcCreateNewThreadSafeModule(mod, tsc);
  LLVMErrorRef e = LLVMOrcLLJITAddLLVMIRModule(run_jit, LLVMOrcLLJITGetMainJITDylib(run_jit), tsm);
  int result = n;
  if (e) {
    char *m = LLVMGetErrorMessage(e);
    snprintf(why, why_len, "the JIT can't add it: %s", m);
    LLVMDisposeErrorMessage(m);
    result = -1;
  }
  // (every new body compiled first, then each slot pointed at its own)
  LLVMOrcExecutorAddress *bodies = calloc(count ? count : 1, sizeof(LLVMOrcExecutorAddress));
  LLVMOrcExecutorAddress *slots = calloc(count ? count : 1, sizeof(LLVMOrcExecutorAddress));
  for (int i = 0; i < count && result >= 0; i++) {
    if (!changed[i]) continue;
    char name[1024];
    snprintf(name, sizeof name, "%s.proven.%d", changed[i], generation);
    e = LLVMOrcLLJITLookup(run_jit, &bodies[i], name);
    if (!e) {
      snprintf(name, sizeof name, "%s.slot", changed[i]);
      e = LLVMOrcLLJITLookup(run_jit, &slots[i], name);
    }
    if (e) {
      char *m = LLVMGetErrorMessage(e);
      snprintf(why, why_len, "the JIT can't compile it: %s", m);
      LLVMDisposeErrorMessage(m);
      result = -1;
    }
  }
  for (int i = 0; i < count && result >= 0; i++)
    if (changed[i])
      atomic_store_explicit((_Atomic(uintptr_t) *)(uintptr_t)slots[i], (uintptr_t)bodies[i], memory_order_release);
  free(bodies);
  free(slots);
  for (int i = 0; i < count; i++) free(changed[i]);
  free(changed);
  return result;
}

static char *read_file(const char *path, size_t *len) {
  FILE *f = fopen(path, "rb");
  if (!f) return NULL;
  size_t cap = 1 << 16, n = 0;
  char *buf = malloc(cap + 1);
  size_t got;
  while ((got = fread(buf + n, 1, cap - n, f)) > 0) {
    n += got;
    if (n == cap) buf = realloc(buf, (cap *= 2) + 1);
  }
  fclose(f);
  buf[n] = 0;
  if (len) *len = n;
  return buf;
}

static char *in_dir(const char *file) {
  size_t n = strlen(alongside_dir) + strlen(file) + 2;
  char *p = malloc(n);
  snprintf(p, n, "%s/%s", alongside_dir, file);
  return p;
}

// The verifier's folder removed (the files it leaves there).
static void remove_dir(void) {
  const char *files[] = {"log", "started.txt", "proofs.txt", "proven.ll", "proven.ll.tmp"};
  for (size_t i = 0; i < sizeof files / sizeof *files; i++) {
    char *p = in_dir(files[i]);
    unlink(p);
    free(p);
  }
  rmdir(alongside_dir);
}

// The verifier waited for, then its proven code swapped in.
static void *watch(void *unused) {
  (void)unused;
  int status;
  while (waitpid(verifier, &status, 0) < 0 && errno == EINTR) {
  }
  char *log = in_dir("log");
  char *said = read_file(log, NULL);
  free(log);
  char *ir_path = in_dir("proven.ll");
  size_t len = 0;
  char *ir = read_file(ir_path, &len);
  free(ir_path);
  pthread_mutex_lock(&swap_lock);
  verifier_said = said;
  if (!ending && ir) {
    char why[512] = "";
    swapped = swap_in(ir, len, why, sizeof why);
    if (swapped < 0 && alongside_notes) fprintf(stderr, "note: the proven code couldn't be swapped in: %s\n", why);
  }
  verified = 1;
  pthread_cond_broadcast(&swap_done);
  pthread_mutex_unlock(&swap_lock);
  free(ir);
  remove_dir();
  return NULL;
}

// When the program ends: how verifying alongside went, on stderr (with
// notes), one summary after everything the program wrote.
static void alongside_end(void) {
  pthread_mutex_lock(&swap_lock);
  ending = 1;
  if (alongside_notes) {
    fflush(stdout);
    if (!verified) {
      fprintf(stderr, "note: still verifying in the background: the next run leaves out the checks it proves\n");
    } else {
      // (verify's own line: `verified N changed: ...`, or nothing new)
      char *said = verifier_said ? verifier_said : "";
      size_t n = strlen(said);
      while (n && (said[n - 1] == '\n' || said[n - 1] == ' ')) said[--n] = 0;
      if (swapped > 0)
        fprintf(stderr, "note: verified alongside (%s): %d function%s swapped to %s proven code while running\n",
                n ? said : "nothing new", swapped, swapped == 1 ? "" : "s", swapped == 1 ? "its" : "their");
      else
        fprintf(stderr, "note: verified alongside (%s)\n", n ? said : "nothing new");
    }
    fflush(stderr);
  }
  pthread_mutex_unlock(&swap_lock);
}

// Starts the verifier: `argv` (newline-separated: this executable,
// `verify-alongside`, ...), its output to `dir`/log, its standard input
// empty, no other descriptor of this process's inherited (it may outlive
// the program, and must not hold its pipes open). The program is then
// compiled swappable. `notes`: the summary when the program ends. 0 when
// started, else 1.
int32_t jit_alongside(const char *argv, const char *dir, int32_t notes) {
  char *copy = strdup(argv);
  char *args[64];
  int n = 0;
  for (char *p = copy, *at = copy; n < 63; p++) {
    if (*p == '\n' || *p == 0) {
      int end = *p == 0;
      *p = 0;
      args[n++] = at;
      at = p + 1;
      if (end) break;
    }
  }
  args[n] = NULL;
  alongside_dir = strdup(dir);
  char *log = in_dir("log");
  posix_spawn_file_actions_t fa;
  posix_spawn_file_actions_init(&fa);
  posix_spawn_file_actions_addopen(&fa, 0, "/dev/null", O_RDONLY, 0);
  posix_spawn_file_actions_addopen(&fa, 1, log, O_WRONLY | O_CREAT | O_TRUNC, 0644);
  posix_spawn_file_actions_adddup2(&fa, 1, 2);
  posix_spawnattr_t attr;
  posix_spawnattr_init(&attr);
#ifdef POSIX_SPAWN_CLOEXEC_DEFAULT
  posix_spawnattr_setflags(&attr, POSIX_SPAWN_CLOEXEC_DEFAULT);
#endif
  int failed_to = posix_spawn(&verifier, args[0], &fa, &attr, args, environ);
  posix_spawn_file_actions_destroy(&fa);
  posix_spawnattr_destroy(&attr);
  free(log);
  free(copy);
  if (failed_to) {
    verifier = 0;
    return 1;
  }
  swappable = 1;
  alongside_notes = notes != 0;
  return 0;
}

// When the program is about to start (compiled): the verifier watched, and
// the summary registered for its end. With STRATA_SWAP_WAIT=start (for
// tests), the program starts only once the proven code is in.
static void alongside_begin(void) {
  if (!verifier) return;
  atexit(alongside_end);
  if (pthread_create(&watcher, NULL, watch, NULL)) return;
  const char *wait = getenv("STRATA_SWAP_WAIT");
  if (wait && !strcmp(wait, "start")) {
    pthread_mutex_lock(&swap_lock);
    while (!verified) pthread_cond_wait(&swap_done, &swap_lock);
    pthread_mutex_unlock(&swap_lock);
  }
}

// The verifier's side: its priority lowered, so that the program it
// verifies alongside runs as fast as it can.
int32_t jit_lower_priority(void) { return setpriority(PRIO_PROCESS, 0, 10); }

// Whether the process `pid` that started this one still waits for it (this
// process wasn't handed to another parent).
int32_t jit_parent_alive(int64_t pid) { return getppid() == (pid_t)pid ? 1 : 0; }
