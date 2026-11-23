// The JIT the compiler written in Strata runs programs with (plan item C20):
// LLVM's LLJIT through its C API. The IR is compiler/codegen's; the
// runtime functions it calls (`strata_*`) are found in this process, the
// compiler's own executable, which runtime.c is linked into.

#include <stddef.h>
// (LLVM's C API, declared here: a package's C has no include path)
typedef struct LLVMOpaqueContext *LLVMContextRef;
typedef struct LLVMOpaqueModule *LLVMModuleRef;
typedef struct LLVMOpaqueMemoryBuffer *LLVMMemoryBufferRef;
typedef struct LLVMOpaqueError *LLVMErrorRef;
typedef struct LLVMOrcOpaqueThreadSafeContext *LLVMOrcThreadSafeContextRef;
typedef struct LLVMOrcOpaqueThreadSafeModule *LLVMOrcThreadSafeModuleRef;
typedef struct LLVMOrcOpaqueLLJIT *LLVMOrcLLJITRef;
typedef struct LLVMOrcOpaqueLLJITBuilder *LLVMOrcLLJITBuilderRef;
typedef struct LLVMOrcOpaqueJITDylib *LLVMOrcJITDylibRef;
typedef struct LLVMOrcOpaqueDefinitionGenerator *LLVMOrcDefinitionGeneratorRef;
typedef unsigned long long LLVMOrcExecutorAddress;
char *LLVMGetErrorMessage(LLVMErrorRef);
void LLVMDisposeErrorMessage(char *);
void LLVMDisposeMessage(char *);
int LLVMInitializeAArch64Target(void);
int LLVMInitializeAArch64TargetInfo(void);
int LLVMInitializeAArch64TargetMC(void);
int LLVMInitializeAArch64AsmPrinter(void);
LLVMContextRef LLVMContextCreate(void);
LLVMOrcThreadSafeContextRef LLVMOrcCreateNewThreadSafeContextFromLLVMContext(LLVMContextRef);
LLVMMemoryBufferRef LLVMCreateMemoryBufferWithMemoryRangeCopy(const char *, size_t, const char *);
int LLVMParseIRInContext(LLVMContextRef, LLVMMemoryBufferRef, LLVMModuleRef *, char **);
LLVMOrcLLJITBuilderRef LLVMOrcCreateLLJITBuilder(void);
LLVMErrorRef LLVMOrcCreateLLJIT(LLVMOrcLLJITRef *, LLVMOrcLLJITBuilderRef);
char LLVMOrcLLJITGetGlobalPrefix(LLVMOrcLLJITRef);
LLVMErrorRef LLVMOrcCreateDynamicLibrarySearchGeneratorForProcess(LLVMOrcDefinitionGeneratorRef *, char, void *, void *);
LLVMOrcJITDylibRef LLVMOrcLLJITGetMainJITDylib(LLVMOrcLLJITRef);
void LLVMOrcJITDylibAddGenerator(LLVMOrcJITDylibRef, LLVMOrcDefinitionGeneratorRef);
LLVMOrcThreadSafeModuleRef LLVMOrcCreateNewThreadSafeModule(LLVMModuleRef, LLVMOrcThreadSafeContextRef);
LLVMErrorRef LLVMOrcLLJITAddLLVMIRModule(LLVMOrcLLJITRef, LLVMOrcJITDylibRef, LLVMOrcThreadSafeModuleRef);
LLVMErrorRef LLVMOrcLLJITLookup(LLVMOrcLLJITRef, LLVMOrcExecutorAddress *, const char *);

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

// Run the IR's `main` with these arguments: its exit code, or 70 when the
// JIT can't run it.
int32_t jit_run_main(const char *ir, const char *argv0) {
  LLVMInitializeAArch64TargetInfo();
  LLVMInitializeAArch64Target();
  LLVMInitializeAArch64TargetMC();
  LLVMInitializeAArch64AsmPrinter();
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
  char *args[] = {(char *)argv0, NULL};
  int code = entry(1, args);
  fflush(stdout);
  return code;
}
