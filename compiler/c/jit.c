// The JIT the compiler written in Strata runs programs with (plan item C20):
// LLVM's LLJIT through its C API. The IR is compiler/codegen's; the
// runtime functions it calls (`strata_*`) are found in this process, the
// compiler's own executable, which runtime.c is linked into.

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

static int failed(LLVMErrorRef e, const char *what) {
  char *m = LLVMGetErrorMessage(e);
  fprintf(stderr, "error: the JIT %s: %s\n", what, m);
  LLVMDisposeErrorMessage(m);
  return 70;
}

// Run the IR's `main` with these arguments (the program's name first, each
// after a newline): its exit code, or 70 when the
// JIT can't run it.
int32_t jit_run_main(const char *ir, const char *args) {
  // (the machine's own target, whichever it is)
  if (LLVMInitializeNativeTarget() || LLVMInitializeNativeAsmPrinter()) {
    fprintf(stderr, "error: the JIT has no target for this machine\n");
    return 70;
  }
  LLVMContextRef ctx = LLVMContextCreate();
  LLVMMemoryBufferRef buf = LLVMCreateMemoryBufferWithMemoryRangeCopy(ir, strlen(ir), "strata");
  LLVMModuleRef mod;
  char *msg = NULL;
  if (LLVMParseIRInContext(ctx, buf, &mod, &msg)) {
    fprintf(stderr, "error: LLVM rejected the generated IR: %s\n", msg ? msg : "");
    LLVMDisposeMessage(msg);
    return 70;
  }
  LLVMOrcLLJITRef jit;
  LLVMErrorRef e = LLVMOrcCreateLLJIT(&jit, LLVMOrcCreateLLJITBuilder());
  if (e) return failed(e, "can't start");
  // the runtime: this process's own symbols
  LLVMOrcDefinitionGeneratorRef gen;
  e = LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess(&gen, LLVMOrcLLJITGetGlobalPrefix(jit), NULL, NULL);
  if (e) return failed(e, "can't see the runtime");
  LLVMOrcJITDylibAddGenerator(LLVMOrcLLJITGetMainJITDylib(jit), gen);
  LLVMOrcThreadSafeContextRef tsc = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(ctx);
  LLVMOrcThreadSafeModuleRef tsm = LLVMOrcCreateNewThreadSafeModule(mod, tsc);
  e = LLVMOrcLLJITAddLLVMIRModule(jit, LLVMOrcLLJITGetMainJITDylib(jit), tsm);
  if (e) return failed(e, "can't add the program");
  LLVMOrcExecutorAddress addr;
  e = LLVMOrcLLJITLookup(jit, &addr, "main");
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

// Load a shared library into this process for the JIT to find its symbols
// (std's precompiled code): 0 when loaded, else 1 (after saying why).
int32_t jit_load(const char *path) {
  if (dlopen(path, RTLD_NOW | RTLD_GLOBAL)) return 0;
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

typedef struct {
  int64_t len;
  char bytes[];
} Str;
// (the runtime's, in this process: looked up when called, since this file
// is also linked where there is no runtime)
static void *runtime_fn(const char *name) { return dlsym(RTLD_DEFAULT, name); }

static LLVMOrcLLJITRef fuzz_jit;
static char *fuzz_error;

// Compile the drivers' IR: 0 when done, else 1 (`jit_fuzz_error` says why).
int32_t jit_fuzz_load(const char *ir) {
  free(fuzz_error);
  fuzz_error = NULL;
  if (LLVMInitializeNativeTarget() || LLVMInitializeNativeAsmPrinter()) {
    fuzz_error = strdup("the JIT has no target for this machine");
    return 1;
  }
  LLVMContextRef ctx = LLVMContextCreate();
  LLVMMemoryBufferRef buf = LLVMCreateMemoryBufferWithMemoryRangeCopy(ir, strlen(ir), "strata-fuzz");
  LLVMModuleRef mod;
  char *msg = NULL;
  if (LLVMParseIRInContext(ctx, buf, &mod, &msg)) {
    fuzz_error = strdup(msg ? msg : "LLVM rejected the generated IR");
    LLVMDisposeMessage(msg);
    return 1;
  }
  LLVMErrorRef e = LLVMOrcCreateLLJIT(&fuzz_jit, LLVMOrcCreateLLJITBuilder());
  if (!e) {
    LLVMOrcDefinitionGeneratorRef gen;
    e = LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess(&gen, LLVMOrcLLJITGetGlobalPrefix(fuzz_jit), NULL, NULL);
    if (!e) {
      LLVMOrcJITDylibAddGenerator(LLVMOrcLLJITGetMainJITDylib(fuzz_jit), gen);
      LLVMOrcThreadSafeContextRef tsc = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(ctx);
      LLVMOrcThreadSafeModuleRef tsm = LLVMOrcCreateNewThreadSafeModule(mod, tsc);
      e = LLVMOrcLLJITAddLLVMIRModule(fuzz_jit, LLVMOrcLLJITGetMainJITDylib(fuzz_jit), tsm);
    }
  }
  if (e) {
    char *m = LLVMGetErrorMessage(e);
    fuzz_error = strdup(m);
    LLVMDisposeErrorMessage(m);
    return 1;
  }
  return 0;
}

const char *jit_fuzz_error(void) { return fuzz_error ? fuzz_error : ""; }

// One case of driver `i`: 0 when it passed, 1 when a fault ended it, -1
// when there is no such driver.
int32_t jit_fuzz_case(int64_t i, int64_t seed, int64_t budget) {
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
