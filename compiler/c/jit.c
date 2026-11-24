// The JIT the compiler written in Strata runs programs with (plan item C20):
// LLVM's LLJIT through its C API. The IR is compiler/codegen's; what it
// declares is found in the libraries loaded first (`jit_load`), then in this
// process, the compiler's own executable, which runtime.c is linked into
// (the runtime's `strata_*` functions).

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
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// The libraries `jit_load` loaded (std's precompiled code, a package's C),
// in order. The JIT finds a symbol in them before it looks in this process:
// the compiler's executable has its own copies of std's functions (it is a
// Strata program too, compiled by Rust's code generator, whose values are
// laid out differently), and those must not stand in for the library's.
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

// How programs and tests are compiled (`jit_set_opt`): -1 as LLVM's JIT
// does by default (its code generator at -O2, no IR passes: what `verify`'s
// fuzzing and compile-time code get), 0 unoptimized (`strata run` and
// `strata test`, as Rust's driver: several times faster to compile), 2 as
// clang -O2 would (`--release`: the IR passes too).
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
// set and not `0`): every function keeps its frame pointer
// ("frame-pointer"="all"), so that a sampling profiler (macOS's `sample`)
// walks through Strata's frames. Off by default: the code is as without it.
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

// Instruction selection by SelectionDAG, and FastISel at -O0, not the
// GlobalISel LLVM 23 turns on for every AArch64 target machine: at -O0 half
// the time for compiler/'s code (4.5 s -> 2.5 s), code as good. Set on each
// target machine (c/toolchain.c's objects too), never as a process-wide
// option: `-global-isel=0` held only for the first module a process
// compiled, and GlobalISel leaves state behind, so an object compiled
// after the JIT (compile-time values, for the host's CPU) differed from the
// same IR compiled alone (an `optnone` function aligned to 64 bytes instead
// of 4): a build's bytes depended on whether its compile-time values were
// cached (IDEAS.md, reproducible builds).
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
  return jit;
}

// Run the IR's `main` with these arguments (the program's name first, each
// after a newline): its exit code, or 70 when the
// JIT can't run it.
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

// Run `strata.call.0`: what the call printed, then its result on a line of
// its own (0); or the
// fault on stderr (1), with its call stack below the probe `probe`; the
// refinement of an argument of `fun` broken on entry is invalid input (2,
// the message on stderr after `error: invalid input: `); 70 when the JIT
// can't run it.
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

// Load a shared library into this process for the JIT to find its symbols
// (std's precompiled code): 0 when loaded, else 1 (after saying why).
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
static LLVMOrcLLJITRef fuzz_jits[256];
static int fuzz_count;
static char *fuzz_error;

// Compile the drivers' IR: its handle (0 and up), else -1 (`jit_fuzz_error`
// says why).
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
  if (fuzz_count == 256) {
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

// Run test `i` of module `m`: 0 when it passed, 1 when a fault ended it,
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

// Call `strata.call.i` of module `m`: 0 when it ended (its value's address
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
