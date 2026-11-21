//! The JIT: `strata run --jit` runs the program's LLVM IR in this process.
//! The IR is the text `compile` writes (one code generator for both): LLVM's
//! libraries parse it, optimize it as clang -O2 would, and compile it to
//! machine code in memory. Its runtime calls go to the C runtime built into
//! strata (build.rs), the one `strata build` links with.

use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{CodeModel, InitializationConfig, RelocMode, Target, TargetMachine};

// The runtime's functions: only their addresses are used, given to LLVM for
// the IR's declarations of the same names.
unsafe extern "C" {
    fn strata_concat();
    fn strata_concat_owned();
    fn strata_host_env_var();
    fn strata_host_args();
    fn strata_prog_name();
    fn strata_options_error();
    fn strata_host_remove();
    fn strata_host_make_dir();
    fn strata_host_list_dir();
    fn strata_host_exists();
    fn strata_host_write_file();
    fn strata_host_read_file();
    fn strata_host_read_line();
    fn strata_host_write();
    fn strata_pid();
    fn strata_int_from_u64();
    fn strata_int_bits();
    fn strata_cstr();
    fn strata_cstr_free();
    fn strata_str_from_c();
    fn strata_init();
    fn strata_seed();
    fn strata_step();
    fn strata_fuzz_arg();
    fn strata_fuzz_args();
    fn strata_fuzz();
    fn strata_test_printed();
    fn strata_test_stack();
    fn strata_test_message();
    fn strata_test_kind();
    fn strata_test();
    fn strata_range();
    fn strata_list_concat();
    fn strata_list_slice();
    fn strata_list_push();
    fn strata_list_new();
    fn strata_sqrt();
    fn strata_float_to_int();
    fn strata_main_error();
    fn strata_str_cmp();
    fn strata_alloc();
    fn strata_dup();
    fn strata_regex_match();
    fn strata_set_regex_engine(e: extern "C" fn(*const u8, i64, *const u8, i64, *mut *const std::ffi::c_char) -> i32);
    fn strata_set_code_host(v: extern "C" fn(*const i64) -> i64, s: extern "C" fn(*const i64, *mut i64) -> i64);
    fn strata_host_code_version();
    fn strata_host_code_snapshot();
    fn strata_trace_call();
    fn strata_trace_return();
    fn strata_ct_keep();
    fn strata_ct_kept() -> i64;
    fn strata_int_dup();
    fn strata_int_drop();
    fn strata_release();
    fn strata_free();
    fn strata_unique();
    fn strata_str_eq();
    fn strata_str_len();
    fn strata_fault();
    fn strata_enter();
    fn strata_leave();
    fn strata_overflow();
    fn strata_div();
    fn strata_gen_string();
    fn strata_gen_len();
    fn strata_gen_float();
    fn strata_gen_int();
    fn strata_random_unit();
    fn strata_random();
    fn strata_int_from_i64();
    fn strata_int_to_float();
    fn strata_int_lit();
    fn strata_int_eq();
    fn strata_int_cmp();
    fn strata_int_mul();
    fn strata_int_sub();
    fn strata_int_add();
    fn strata_mod();
    fn strata_fmod();
    fn strata_finite();
    fn strata_show_int();
    fn strata_json_int();
    fn strata_show_float();
    fn strata_show_bool();
    fn strata_json_str();
    fn strata_debug_str();
    fn strata_print();
    fn strata_str_split();
    fn strata_str_trim();
    fn strata_str_contains();
    fn strata_str_starts_with();
    fn strata_str_ends_with();
    fn strata_str_index_of();
    fn strata_str_reverse();
    fn strata_str_chars();
    fn strata_str_at();
    fn strata_str_at_or_fault();
    fn strata_str_slice();
    fn strata_str_replace();
    fn strata_str_repeat();
    fn strata_str_case();
    fn strata_str_is();
    fn strata_dfa_match();
    fn strata_str_ord();
    fn strata_str_chr();
    fn strata_parse_int();
    fn strata_parse_float();
    fn strata_show_float_fixed();
    fn strata_format();
}

fn runtime() -> Vec<(&'static str, usize)> {
    vec![
        ("strata_concat", strata_concat as *const () as usize),
        ("strata_int_from_u64", strata_int_from_u64 as *const () as usize),
        ("strata_int_bits", strata_int_bits as *const () as usize),
        ("strata_cstr", strata_cstr as *const () as usize),
        ("strata_cstr_free", strata_cstr_free as *const () as usize),
        ("strata_str_from_c", strata_str_from_c as *const () as usize),
        ("strata_concat_owned", strata_concat_owned as *const () as usize),
        ("strata_host_env_var", strata_host_env_var as *const () as usize),
        ("strata_host_args", strata_host_args as *const () as usize),
        ("strata_prog_name", strata_prog_name as *const () as usize),
        ("strata_options_error", strata_options_error as *const () as usize),
        ("strata_host_remove", strata_host_remove as *const () as usize),
        ("strata_host_make_dir", strata_host_make_dir as *const () as usize),
        ("strata_host_list_dir", strata_host_list_dir as *const () as usize),
        ("strata_host_exists", strata_host_exists as *const () as usize),
        ("strata_host_write_file", strata_host_write_file as *const () as usize),
        ("strata_host_read_file", strata_host_read_file as *const () as usize),
        ("strata_host_code_version", strata_host_code_version as *const () as usize),
        ("strata_host_code_snapshot", strata_host_code_snapshot as *const () as usize),
        ("strata_host_read_line", strata_host_read_line as *const () as usize),
        ("strata_host_write", strata_host_write as *const () as usize),
        ("strata_pid", strata_pid as *const () as usize),
        ("strata_init", strata_init as *const () as usize),
        ("strata_seed", strata_seed as *const () as usize),
        ("strata_step", strata_step as *const () as usize),
        ("strata_fuzz_arg", strata_fuzz_arg as *const () as usize),
        ("strata_fuzz_args", strata_fuzz_args as *const () as usize),
        ("strata_fuzz", strata_fuzz as *const () as usize),
        ("strata_test_printed", strata_test_printed as *const () as usize),
        ("strata_test_stack", strata_test_stack as *const () as usize),
        ("strata_test_message", strata_test_message as *const () as usize),
        ("strata_test_kind", strata_test_kind as *const () as usize),
        ("strata_test", strata_test as *const () as usize),
        ("strata_range", strata_range as *const () as usize),
        ("strata_list_concat", strata_list_concat as *const () as usize),
        ("strata_list_slice", strata_list_slice as *const () as usize),
        ("strata_list_push", strata_list_push as *const () as usize),
        ("strata_list_new", strata_list_new as *const () as usize),
        ("strata_sqrt", strata_sqrt as *const () as usize),
        ("strata_float_to_int", strata_float_to_int as *const () as usize),
        ("strata_main_error", strata_main_error as *const () as usize),
        ("strata_str_cmp", strata_str_cmp as *const () as usize),
        ("strata_alloc", strata_alloc as *const () as usize),
        ("strata_dup", strata_dup as *const () as usize),
        ("strata_regex_match", strata_regex_match as *const () as usize),
        ("strata_trace_call", strata_trace_call as *const () as usize),
        ("strata_trace_return", strata_trace_return as *const () as usize),
        ("strata_ct_keep", strata_ct_keep as *const () as usize),
        ("strata_int_dup", strata_int_dup as *const () as usize),
        ("strata_int_drop", strata_int_drop as *const () as usize),
        ("strata_release", strata_release as *const () as usize),
        ("strata_free", strata_free as *const () as usize),
        ("strata_unique", strata_unique as *const () as usize),
        ("strata_str_eq", strata_str_eq as *const () as usize),
        ("strata_str_len", strata_str_len as *const () as usize),
        ("strata_fault", strata_fault as *const () as usize),
        ("strata_enter", strata_enter as *const () as usize),
        ("strata_leave", strata_leave as *const () as usize),
        ("strata_overflow", strata_overflow as *const () as usize),
        ("strata_div", strata_div as *const () as usize),
        ("strata_gen_string", strata_gen_string as *const () as usize),
        ("strata_gen_len", strata_gen_len as *const () as usize),
        ("strata_gen_float", strata_gen_float as *const () as usize),
        ("strata_gen_int", strata_gen_int as *const () as usize),
        ("strata_random_unit", strata_random_unit as *const () as usize),
        ("strata_random", strata_random as *const () as usize),
        ("strata_int_from_i64", strata_int_from_i64 as *const () as usize),
        ("strata_int_to_float", strata_int_to_float as *const () as usize),
        ("strata_int_lit", strata_int_lit as *const () as usize),
        ("strata_int_eq", strata_int_eq as *const () as usize),
        ("strata_int_cmp", strata_int_cmp as *const () as usize),
        ("strata_int_mul", strata_int_mul as *const () as usize),
        ("strata_int_sub", strata_int_sub as *const () as usize),
        ("strata_int_add", strata_int_add as *const () as usize),
        ("strata_mod", strata_mod as *const () as usize),
        ("strata_fmod", strata_fmod as *const () as usize),
        ("strata_finite", strata_finite as *const () as usize),
        ("strata_show_int", strata_show_int as *const () as usize),
        ("strata_json_int", strata_json_int as *const () as usize),
        ("strata_show_float", strata_show_float as *const () as usize),
        ("strata_show_bool", strata_show_bool as *const () as usize),
        ("strata_json_str", strata_json_str as *const () as usize),
        ("strata_debug_str", strata_debug_str as *const () as usize),
        ("strata_print", strata_print as *const () as usize),
        ("strata_str_split", strata_str_split as *const () as usize),
        ("strata_str_trim", strata_str_trim as *const () as usize),
        ("strata_str_contains", strata_str_contains as *const () as usize),
        ("strata_str_starts_with", strata_str_starts_with as *const () as usize),
        ("strata_str_ends_with", strata_str_ends_with as *const () as usize),
        ("strata_str_index_of", strata_str_index_of as *const () as usize),
        ("strata_str_reverse", strata_str_reverse as *const () as usize),
        ("strata_str_chars", strata_str_chars as *const () as usize),
        ("strata_str_at", strata_str_at as *const () as usize),
        ("strata_str_at_or_fault", strata_str_at_or_fault as *const () as usize),
        ("strata_str_slice", strata_str_slice as *const () as usize),
        ("strata_str_replace", strata_str_replace as *const () as usize),
        ("strata_str_repeat", strata_str_repeat as *const () as usize),
        ("strata_str_case", strata_str_case as *const () as usize),
        ("strata_str_is", strata_str_is as *const () as usize),
        ("strata_dfa_match", strata_dfa_match as *const () as usize),
        ("strata_str_ord", strata_str_ord as *const () as usize),
        ("strata_str_chr", strata_str_chr as *const () as usize),
        ("strata_parse_int", strata_parse_int as *const () as usize),
        ("strata_parse_float", strata_parse_float as *const () as usize),
        ("strata_show_float_fixed", strata_show_float_fixed as *const () as usize),
        ("strata_format", strata_format as *const () as usize),
    ]
}

/// How a test ran: the fault that ended it (kind, message, call stack), if
/// one did, and what it printed.
pub struct TestOutcome {
    pub fault: Option<(String, String, String)>,
    pub printed: String,
}

/// Run the tests of the IR `compile_tests` made, each in turn: test `i` is
/// `strata.test.i`. A fault ends a test, not the process.
pub fn run_tests(ir: &str, which: &[usize]) -> Result<Vec<TestOutcome>, String> {
    use std::ffi::{c_char, CStr};
    let ctx = Context::create();
    let engine = engine(&ctx, ir)?;
    type Init = unsafe extern "C" fn(i32, *const *const c_char);
    type Test = unsafe extern "C" fn(unsafe extern "C" fn()) -> i32;
    type Text = unsafe extern "C" fn() -> *const c_char;
    type Message = unsafe extern "C" fn() -> *const i64;
    let (init, test, kind, message, stack, printed): (Init, Test, Text, Message, Text, Text) = unsafe {
        (
            std::mem::transmute(strata_init as *const ()),
            std::mem::transmute(strata_test as *const ()),
            std::mem::transmute(strata_test_kind as *const ()),
            std::mem::transmute(strata_test_message as *const ()),
            std::mem::transmute(strata_test_stack as *const ()),
            std::mem::transmute(strata_test_printed as *const ()),
        )
    };
    let name = std::ffi::CString::new("test").unwrap();
    let argv = [name.as_ptr()];
    unsafe { init(1, argv.as_ptr()) };
    let text = |p: *const c_char| unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string();
    let mut out = vec![];
    for &i in which {
        let addr = engine.get_function_address(&format!("strata.test.{i}")).map_err(|e| e.to_string())?;
        let f: unsafe extern "C" fn() = unsafe { std::mem::transmute(addr) };
        let failed = unsafe { test(f) } != 0;
        let fault = failed.then(|| {
            // the message: a runtime string (its length, then its bytes)
            let m = unsafe { message() };
            let msg = unsafe { std::slice::from_raw_parts(m.add(1) as *const u8, *m as usize) };
            (text(unsafe { kind() }), String::from_utf8_lossy(msg).to_string(), text(unsafe { stack() }))
        });
        out.push(TestOutcome { fault, printed: text(unsafe { printed() }) });
    }
    Ok(out)
}

/// The fuzz drivers of IR `compile_fuzz` made, compiled for good (the
/// engine lives as long as the process): their addresses, in order.
pub fn fuzzers(ir: &str, n: usize) -> Result<Vec<usize>, String> {
    use std::ffi::c_char;
    let ctx: &'static Context = Box::leak(Box::new(Context::create()));
    // unoptimized: compiling the drivers costs more than optimizing saves on a few hundred cases
    let engine = engine_at(ctx, ir, false)?;
    let addrs = (0..n).map(|i| engine.get_function_address(&format!("strata.fuzz.{i}")).map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
    std::mem::forget(engine);
    type Init = unsafe extern "C" fn(i32, *const *const c_char);
    let init: Init = unsafe { std::mem::transmute(strata_init as *const ()) };
    let name = std::ffi::CString::new("verify").unwrap();
    let argv = [name.as_ptr()];
    unsafe { init(1, argv.as_ptr()) };
    Ok(addrs)
}

/// One fuzzing case of the driver at `addr`, seeded, within `budget` steps:
/// the fault that ended it (kind, message, call stack), if any, and its
/// arguments as JSON. (Not reentrant: the runtime's state is global.)
pub fn fuzz_case(addr: usize, seed: u64, budget: i64) -> (Option<(String, String, String)>, String) {
    use std::ffi::{c_char, CStr};
    type Fuzz = unsafe extern "C" fn(unsafe extern "C" fn(), i64, i64) -> i32;
    type Text = unsafe extern "C" fn() -> *const c_char;
    type Message = unsafe extern "C" fn() -> *const i64;
    let (fuzz, kind, message, stack, args): (Fuzz, Text, Message, Text, Text) = unsafe {
        (
            std::mem::transmute(strata_fuzz as *const ()),
            std::mem::transmute(strata_test_kind as *const ()),
            std::mem::transmute(strata_test_message as *const ()),
            std::mem::transmute(strata_test_stack as *const ()),
            std::mem::transmute(strata_fuzz_args as *const ()),
        )
    };
    let text = |p: *const c_char| unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string();
    let f: unsafe extern "C" fn() = unsafe { std::mem::transmute(addr) };
    let failed = unsafe { fuzz(f, seed as i64, budget) } != 0;
    let fault = failed.then(|| {
        let m = unsafe { message() };
        let msg = unsafe { std::slice::from_raw_parts(m.add(1) as *const u8, *m as usize) };
        (text(unsafe { kind() }), String::from_utf8_lossy(msg).to_string(), text(unsafe { stack() }))
    });
    (fault, text(unsafe { args() }))
}

/// A fault that ended compiled code: its kind (`Refinement`, `Limit`, ...),
/// message and call stack (as `strata run` shows it, "" when empty).
#[derive(Debug, Clone)]
pub struct Fault {
    pub kind: String,
    pub message: String,
    pub stack: String,
}

/// Run compile-time code (`llvm::compile_comptime`): each `strata.ct.N`,
/// within `budget` steps, its value read (`native::read`) before the code
/// is gone; a fault or the budget's end is its message.
pub fn comptime(ir: &str, roots: &[super::Shape], shapes: &super::Shapes, budget: i64) -> Result<Vec<Result<crate::interp::Value, Fault>>, String> {
    Ok(run_kept(ir, roots, shapes, budget, false)?.into_iter().map(|(r, _)| r).collect())
}

/// `comptime`, optionally optimized (`strata run f [args]`: what it runs may
/// be long), with what each one printed.
pub fn run_kept(ir: &str, roots: &[super::Shape], shapes: &super::Shapes, budget: i64, optimize: bool) -> Result<Vec<(Result<crate::interp::Value, Fault>, String)>, String> {
    use std::ffi::{c_char, CStr};
    let ctx = Context::create();
    let engine = engine_at(&ctx, ir, optimize)?;
    type Init = unsafe extern "C" fn(i32, *const *const c_char);
    type Fuzz = unsafe extern "C" fn(unsafe extern "C" fn(), i64, i64) -> i32;
    type Message = unsafe extern "C" fn() -> *const i64;
    type Text = unsafe extern "C" fn() -> *const c_char;
    let (init, fuzz, message, kind, stack, printed): (Init, Fuzz, Message, Text, Text, Text) = unsafe {
        (
            std::mem::transmute(strata_init as *const ()),
            std::mem::transmute(strata_fuzz as *const ()),
            std::mem::transmute(strata_test_message as *const ()),
            std::mem::transmute(strata_test_kind as *const ()),
            std::mem::transmute(strata_test_stack as *const ()),
            std::mem::transmute(strata_test_printed as *const ()),
        )
    };
    let text = |p: *const c_char| unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string();
    let name = std::ffi::CString::new("comptime").unwrap();
    let argv = [name.as_ptr()];
    unsafe { init(1, argv.as_ptr()) };
    let mut out = vec![];
    for (i, root) in roots.iter().enumerate() {
        let addr = engine.get_function_address(&format!("strata.ct.{i}")).map_err(|e| e.to_string())?;
        let f: unsafe extern "C" fn() = unsafe { std::mem::transmute(addr) };
        if unsafe { fuzz(f, 1, budget) } != 0 {
            let m = unsafe { message() };
            let msg = unsafe { std::slice::from_raw_parts(m.add(1) as *const u8, *m as usize) };
            out.push((Err(Fault { kind: text(unsafe { kind() }), message: String::from_utf8_lossy(msg).to_string(), stack: text(unsafe { stack() }) }), text(unsafe { printed() })));
            continue;
        }
        let v = unsafe { super::native::read(root, shapes, strata_ct_kept()) }.map_err(|m| Fault { kind: "Runtime".into(), message: format!("reading the value: {m}"), stack: String::new() });
        out.push((v, text(unsafe { printed() })));
    }
    Ok(out)
}

/// Run the IR's `main` with the program's arguments; its exit code. A fault
/// exits the process from the runtime, as a compiled program does.
/// The IR compiled in memory (optimized as clang -O2 would), its runtime
/// calls mapped to the runtime built into strata.
fn engine<'ctx>(ctx: &'ctx Context, ir: &str) -> Result<inkwell::execution_engine::ExecutionEngine<'ctx>, String> {
    engine_at(ctx, ir, true)
}

/// `engine`, optimized or not: code run once (compile-time code, a probe)
/// costs less to compile unoptimized than it would save.
/// The runtime's engine for patterns built at run time: Rust's `regex`, as
/// the interpreter's `matches` (the whole string, `^(?:re)$`), each pattern
/// compiled once.
extern "C" fn regex_engine(s: *const u8, n: i64, re: *const u8, m: i64, err: *mut *const std::ffi::c_char) -> i32 {
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<String, Result<regex::Regex, std::ffi::CString>>> = Default::default();
    }
    let (s, re) = unsafe { (std::slice::from_raw_parts(s, n as usize), std::slice::from_raw_parts(re, m as usize)) };
    let (s, re) = (String::from_utf8_lossy(s), String::from_utf8_lossy(re).to_string());
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let r = c.entry(re.clone()).or_insert_with(|| regex::Regex::new(&format!("^(?:{re})$")).map_err(|e| std::ffi::CString::new(format!("bad regex: {e}")).unwrap_or_default()));
        match r {
            Ok(r) => r.is_match(&s) as i32,
            Err(m) => {
                unsafe { *err = m.as_ptr() };
                -1
            }
        }
    })
}

/// std's non-generic functions, compiled once at O2 (`llvm::compile_library`)
/// and kept in a cache by std's code and this strata: a shared library the
/// JIT loads (its calls into the runtime resolve to strata's own, exported),
/// and an object file `strata build` links statically. `None` without a C
/// compiler, or with `STRATA_NO_STD_LIBRARY`.
pub struct StdLibrary {
    pub names: std::collections::HashSet<String>,
    pub object: std::path::PathBuf,
    handle: usize,
}

pub fn std_library() -> Option<&'static StdLibrary> {
    static LIB: std::sync::OnceLock<Option<StdLibrary>> = std::sync::OnceLock::new();
    LIB.get_or_init(|| {
        if std::env::var_os("STRATA_NO_STD_LIBRARY").is_some() {
            return None;
        }
        let r = make_std_library();
        if let Err(e) = &r {
            if std::env::var_os("STRATA_DEBUG").is_some() {
                eprintln!("note: std is compiled with each program: {e}");
            }
        }
        r.ok()
    })
    .as_ref()
}

fn make_std_library() -> Result<StdLibrary, String> {
    use sha2::{Digest, Sha256};
    let std = crate::stdlib::program();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let meta = std::fs::metadata(&exe).map_err(|e| e.to_string())?;
    let key: String = Sha256::digest(format!("v2{:?}{}{:?}", meta.modified().ok(), meta.len(), std).as_bytes()).iter().take(16).map(|b| format!("{b:02x}")).collect();
    let root = match std::env::var_os("STRATA_CACHE") {
        Some(d) => std::path::PathBuf::from(d),
        None => std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache/strata"),
    };
    let dir = root.join("std").join(&key);
    let (lib, obj, names_file) = (dir.join("libstd.so"), dir.join("std.o"), dir.join("names"));
    if !names_file.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // what can be precompiled; what doesn't compile yet is left to each program
        let resolved = crate::resolve::resolve(std).program;
        let (env, _) = crate::types::Env::build(&resolved);
        // a function reading a package's setting (`setting("json.unknown_keys")`,
        // written in per program), or calling one that does, stays per program
        let raw_fns: Vec<(String, &crate::ast::FnDecl)> = std.decls.iter().filter_map(|d| match &d.kind {
            crate::ast::DeclKind::Fn(f) => Some((crate::names::short(&f.name).to_string(), f)),
            _ => None,
        }).collect();
        let mut per_program: std::collections::HashSet<String> = ["setting".to_string()].into();
        loop {
            let before = per_program.len();
            for (n, f) in &raw_fns {
                let mut all = vec![&f.body];
                all.extend(f.requires.iter());
                all.extend(f.ensures.iter());
                if all.iter().any(|b| crate::ast::any_expr(b, |e| matches!(e, crate::ast::Expr::Var(v) if per_program.contains(crate::names::short(v))))) {
                    per_program.insert(n.clone());
                }
            }
            if per_program.len() == before {
                break;
            }
        }
        let mut names: Vec<String> = env.fns.values().filter(|f| super::precompilable(f, &env) && !per_program.contains(crate::names::short(&f.name))).map(|f| f.name.to_string()).collect();
        names.sort();
        let ir = match super::compile_library(std, &names) {
            Ok(ir) => ir,
            Err(_) => {
                names.retain(|n| super::compile_library(std, std::slice::from_ref(n)).is_ok());
                super::compile_library(std, &names)?
            }
        };
        let ll = dir.join("std.ll");
        std::fs::write(&ll, ir).map_err(|e| e.to_string())?;
        let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
        let run = |args: &[&std::ffi::OsStr]| -> Result<(), String> {
            let o = std::process::Command::new(&cc).args(args).output().map_err(|e| format!("{cc}: {e}"))?;
            if o.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&o.stderr).to_string()) }
        };
        let os = |s: &str| std::ffi::OsString::from(s);
        let mut shared = vec![os("-O2"), os("-fPIC"), os("-shared"), os("-Wno-override-module")];
        if cfg!(target_os = "macos") {
            shared.extend([os("-undefined"), os("dynamic_lookup")]);
        }
        shared.extend([os("-o"), lib.clone().into(), ll.clone().into()]);
        run(&shared.iter().map(|s| s.as_os_str()).collect::<Vec<_>>())?;
        run(&[os("-O2").as_os_str(), os("-fPIC").as_os_str(), os("-Wno-override-module").as_os_str(), os("-c").as_os_str(), os("-o").as_os_str(), obj.as_os_str(), ll.as_os_str()])?;
        std::fs::write(&names_file, names.join("\n")).map_err(|e| e.to_string())?;
    }
    let names = std::fs::read_to_string(&names_file).map_err(|e| e.to_string())?.lines().map(String::from).collect();
    let path = std::ffi::CString::new(lib.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
    unsafe extern "C" {
        fn dlopen(path: *const std::ffi::c_char, mode: i32) -> *mut std::ffi::c_void;
    }
    // RTLD_NOW
    let handle = unsafe { dlopen(path.as_ptr(), 2) };
    if handle.is_null() {
        return Err(format!("can't load {}", lib.display()));
    }
    Ok(StdLibrary { names, object: obj, handle: handle as usize })
}

/// The address of a precompiled std function, by its symbol (without `@"..."`).
fn std_symbol(name: &str) -> Option<usize> {
    unsafe extern "C" {
        fn dlsym(handle: *mut std::ffi::c_void, symbol: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    }
    let lib = std_library()?;
    let s = std::ffi::CString::new(name).ok()?;
    let a = unsafe { dlsym(lib.handle as *mut std::ffi::c_void, s.as_ptr()) };
    (!a.is_null()).then_some(a as usize)
}

/// A string compiled code passed (borrowed).
unsafe fn borrowed_str(p: *const i64) -> String {
    unsafe {
        let len = *p as usize;
        String::from_utf8_lossy(std::slice::from_raw_parts(p.add(1) as *const u8, len)).into_owned()
    }
}

/// `Code.version()` for compiled code (std.code): the codebase's fingerprint.
extern "C" fn code_version(root: *const i64) -> i64 {
    let root = unsafe { borrowed_str(root) };
    super::native::new_string(&crate::ui::code_version(std::path::Path::new(&root)))
}

/// `Code.snapshot()` for compiled code: the snapshot (src/ui.rs) read against
/// `std.code.Snapshot` and written into compiled memory, or 0 and the
/// failure in `*err`.
extern "C" fn code_snapshot(root: *const i64, err: *mut i64) -> i64 {
    let root = unsafe { borrowed_str(root) };
    let fail = |m: String| {
        unsafe { *err = super::native::new_string(&m) };
        0
    };
    let j = match crate::ui::code_snapshot(std::path::Path::new(&root)) {
        Ok(j) => j,
        Err(e) => return fail(e),
    };
    // std's types, to read the snapshot against
    static STD: std::sync::OnceLock<crate::types::Env> = std::sync::OnceLock::new();
    let env = STD.get_or_init(|| crate::types::check_and_elaborate(crate::stdlib::program()).env);
    let mut it = crate::interp::Interp::new(env, 1);
    let t = crate::ast::TypeExpr::Named("std.code.Snapshot".into(), vec![]);
    match it.from_json(&t, &j, "snapshot").map_err(|e| format!("the codebase's snapshot doesn't fit `std.code.Snapshot`: {e}")).and_then(|v| super::native::write(&v)) {
        Ok(p) => p,
        Err(e) => fail(e),
    }
}

fn engine_at<'ctx>(ctx: &'ctx Context, ir: &str, optimize: bool) -> Result<inkwell::execution_engine::ExecutionEngine<'ctx>, String> {
    unsafe { strata_set_regex_engine(regex_engine) };
    unsafe { strata_set_code_host(code_version, code_snapshot) };
    Target::initialize_native(&InitializationConfig::default())?;
    let buf = MemoryBuffer::create_from_memory_range_copy(ir.as_bytes(), "strata");
    let module = ctx.create_module_from_ir(buf).map_err(|e| format!("LLVM rejected the generated IR: {e}"))?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
    let cpu = TargetMachine::get_host_cpu_name();
    let features = TargetMachine::get_host_cpu_features();
    let machine = target
        .create_target_machine(&triple, &cpu.to_string(), &features.to_string(), OptimizationLevel::Aggressive, RelocMode::Default, CodeModel::JITDefault)
        .ok_or("LLVM has no target machine for this host")?;
    module.set_triple(&triple);
    module.set_data_layout(&machine.get_target_data().get_data_layout());
    if optimize {
        module.run_passes("default<O2>", &machine, PassBuilderOptions::create()).map_err(|e| e.to_string())?;
    }
    native()?;
    let level = if optimize { OptimizationLevel::Aggressive } else { OptimizationLevel::None };
    let engine = module.create_jit_execution_engine(level).map_err(|e| e.to_string())?;
    let table = runtime();
    // a runtime function left out of the table would be a call to address 0
    for f in module.get_functions() {
        let name = f.get_name().to_string_lossy();
        if f.count_basic_blocks() == 0 && name.starts_with("strata_") && !table.iter().any(|(n, _)| *n == name) {
            return Err(format!("the JIT does not know the runtime's `{name}` (add it to `runtime()` in src/llvm/jit.rs)"));
        }
    }
    for (name, addr) in table {
        if let Some(f) = module.get_function(name) {
            engine.add_global_mapping(&f, addr);
        }
    }
    // std's precompiled functions: the library's code, loaded once
    for f in module.get_functions() {
        let name = f.get_name().to_string_lossy().to_string();
        if f.count_basic_blocks() == 0 && name.starts_with("strata:std.") {
            let addr = std_symbol(&name).ok_or(format!("std's library has no `{name}`"))?;
            engine.add_global_mapping(&f, addr);
        }
    }
    Ok(engine)
}

/// The package's C (`[native]` in strata.toml), for its `extern c`
/// declarations: compiled by clang into a shared library, linked with its
/// libraries, and loaded into this process once. Other C symbols (libc's)
/// are found in the process.
fn native() -> Result<(), String> {
    static LOADED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    LOADED
        .get_or_init(|| {
            let Some((dir, native)) = crate::package::native() else { return Ok(()) };
            let tmp = std::env::temp_dir().join(format!("strata-native-{}", std::process::id()));
            std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
            // an empty file, so libraries alone still make a library
            let empty = tmp.join("empty.c");
            std::fs::write(&empty, "").map_err(|e| e.to_string())?;
            let lib = tmp.join(if cfg!(target_os = "macos") { "native.dylib" } else { "native.so" });
            let cc = std::env::var("STRATA_CC").unwrap_or_else(|_| "clang".into());
            let out = std::process::Command::new(&cc)
                .args(["-shared", "-fPIC", "-O2", "-o"])
                .arg(&lib)
                .arg(&empty)
                .args(native.clang_args(dir))
                .output()
                .map_err(|e| format!("can't run `{cc}` to compile the package's C ([native] in strata.toml): {e}"))?;
            if !out.status.success() {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(format!("`{cc}` failed on the package's C ([native] in strata.toml):\n{}", String::from_utf8_lossy(&out.stderr).trim_end()));
            }
            let loaded = inkwell::support::load_library_permanently(&lib).map_err(|e| format!("can't load the package's C: {e:?}"));
            let _ = std::fs::remove_dir_all(&tmp);
            loaded
        })
        .clone()
}

pub fn run(ir: &str, program: &str, args: &[String]) -> Result<i32, String> {
    let ctx = Context::create();
    let engine = engine(&ctx, ir)?;
    let main = unsafe { engine.get_function::<unsafe extern "C" fn(i32, *const *const std::ffi::c_char) -> i32>("main") }.map_err(|e| e.to_string())?;
    // argv as C has it: the program's name, then its arguments
    let owned: Vec<std::ffi::CString> = std::iter::once(program).chain(args.iter().map(|a| a.as_str())).map(|a| std::ffi::CString::new(a).unwrap_or_default()).collect();
    let argv: Vec<*const std::ffi::c_char> = owned.iter().map(|a| a.as_ptr()).collect();
    Ok(unsafe { main.call(argv.len() as i32, argv.as_ptr()) })
}
