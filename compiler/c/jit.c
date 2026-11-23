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
  // the runtime: this process's own symbols
  LLVMOrcDefinitionGeneratorRef gen;
  e = LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess(&gen, LLVMOrcLLJITGetGlobalPrefix(jit), NULL, NULL);
  if (e) return failed(e, "can't see the runtime"), NULL;
  LLVMOrcJITDylibAddGenerator(LLVMOrcLLJITGetMainJITDylib(jit), gen);
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

// (the runtime's, in this process: looked up when called)
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
  if (dlopen(path, RTLD_NOW | RTLD_GLOBAL)) return 0;
  fprintf(stderr, "note: can't load %s: %s\n", path, dlerror());
  return 1;
}
