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
#include <llvm-c/Target.h>
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
  LLVMContextRef ctx = LLVMContextCreate();
  LLVMMemoryBufferRef buf = LLVMCreateMemoryBufferWithMemoryRangeCopy(ir, strlen(ir), "strata");
  LLVMModuleRef mod;
  char *msg = NULL;
  if (LLVMParseIRInContext(ctx, buf, &mod, &msg)) {
    fprintf(stderr, "error: LLVM rejected the generated IR: %s\n", msg ? msg : "");
    LLVMDisposeMessage(msg);
    return NULL;
  }
  LLVMOrcLLJITRef jit;
  LLVMErrorRef e = LLVMOrcCreateLLJIT(&jit, LLVMOrcCreateLLJITBuilder());
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
  LLVMErrorRef e = LLVMOrcLLJITLookup(jit, &addr, "main");
  if (e) return failed(e, "has no main");
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
  LLVMErrorRef e = LLVMOrcLLJITLookup(jit, &addr, "strata.call.0");
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
  LLVMContextRef ctx = LLVMContextCreate();
  LLVMMemoryBufferRef buf = LLVMCreateMemoryBufferWithMemoryRangeCopy(ir, strlen(ir), "strata-fuzz");
  LLVMModuleRef mod;
  char *msg = NULL;
  if (LLVMParseIRInContext(ctx, buf, &mod, &msg)) {
    fuzz_error = strdup(msg ? msg : "LLVM rejected the generated IR");
    LLVMDisposeMessage(msg);
    return -1;
  }
  if (fuzz_count == 256) {
    fuzz_error = strdup("too many modules compiled");
    return -1;
  }
  LLVMOrcLLJITRef fuzz_jit;
  LLVMErrorRef e = LLVMOrcCreateLLJIT(&fuzz_jit, LLVMOrcCreateLLJITBuilder());
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
