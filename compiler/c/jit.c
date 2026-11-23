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
