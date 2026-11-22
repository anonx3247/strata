//! The JIT: `strata run --jit` runs the program's LLVM IR in this process.
//! The IR is the text `compile` writes (one code generator for both): LLVM's
//! libraries parse it, optimize it as clang -O2 would, and compile it to
//! machine code in memory. Its runtime calls go to the C runtime built into
//! strata (build.rs), the one `strata build` links with.

use crate::ast::Expr;
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
    fn strata_host_read_bytes();
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
    fn strata_dup();
    fn strata_set_code_host(v: extern "C" fn() -> i64, s: extern "C" fn(*mut i64) -> i64);
    fn strata_host_code_version();
    fn strata_host_code_snapshot();
    fn strata_given();
    pub(super) fn strata_set_given(g: *const i64);
    fn strata_ct_keep();
    fn strata_ct_kept() -> i64;
    fn strata_int_dup();
    fn strata_int_drop();
    fn strata_release();
    fn strata_free();
    fn strata_unique();
    fn strata_scope_begin();
    fn strata_scope_end();
    fn strata_spawn();
    fn strata_await();
    fn strata_channel_new();
    fn strata_channel_drop();
    fn strata_channel_send();
    fn strata_channel_recv();
    fn strata_channel_close();
    fn strata_select();
    fn strata_timeline_new();
    fn strata_timeline_drop();
    fn strata_timeline_now();
    fn strata_timeline_set();
    fn strata_timeline_advance();
    fn strata_timeline_sleep();
    fn strata_host_now();
    fn strata_host_sleep();
    fn strata_cancelled();
    fn strata_panic();
    fn strata_was_unwound();
    fn strata_unwound();
    fn strata_scope_cancel();
    static strata_cancel_any: i32;
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
    fn strata_int_and();
    fn strata_int_or();
    fn strata_int_xor();
    fn strata_shl();
    fn strata_shr();
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
    fn strata_str_ord();
    fn strata_str_byte_len();
    fn strata_str_byte_at();
    fn strata_str_byte_slice();
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
        ("strata_given", strata_given as *const () as usize),
        ("strata_host_read_bytes", strata_host_read_bytes as *const () as usize),
        ("strata_host_read_line", strata_host_read_line as *const () as usize),
        ("strata_host_write", strata_host_write as *const () as usize),
        ("strata_pid", strata_pid as *const () as usize),
        ("strata_init", strata_init as *const () as usize),
        ("strata_seed", strata_seed as *const () as usize),
        ("strata_step", strata_step as *const () as usize),
        ("strata_fuzz_keep", fuzz_keep as *const () as usize),
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
        ("strata_alloc", super::native::strata_alloc as *const () as usize),
        ("strata_dup", strata_dup as *const () as usize),
        ("strata_trace_arg", trace_arg as *const () as usize),
        ("strata_trace_call", trace_call as *const () as usize),
        ("strata_trace_return", trace_return as *const () as usize),
        ("strata_ct_keep", strata_ct_keep as *const () as usize),
        ("strata_int_dup", strata_int_dup as *const () as usize),
        ("strata_int_drop", strata_int_drop as *const () as usize),
        ("strata_release", strata_release as *const () as usize),
        ("strata_free", strata_free as *const () as usize),
        ("strata_unique", strata_unique as *const () as usize),
        ("strata_scope_begin", strata_scope_begin as *const () as usize),
        ("strata_scope_end", strata_scope_end as *const () as usize),
        ("strata_spawn", strata_spawn as *const () as usize),
        ("strata_await", strata_await as *const () as usize),
        ("strata_channel_new", strata_channel_new as *const () as usize),
        ("strata_channel_drop", strata_channel_drop as *const () as usize),
        ("strata_channel_send", strata_channel_send as *const () as usize),
        ("strata_channel_recv", strata_channel_recv as *const () as usize),
        ("strata_channel_close", strata_channel_close as *const () as usize),
        ("strata_select", strata_select as *const () as usize),
        ("strata_timeline_new", strata_timeline_new as *const () as usize),
        ("strata_timeline_drop", strata_timeline_drop as *const () as usize),
        ("strata_timeline_now", strata_timeline_now as *const () as usize),
        ("strata_timeline_set", strata_timeline_set as *const () as usize),
        ("strata_timeline_advance", strata_timeline_advance as *const () as usize),
        ("strata_timeline_sleep", strata_timeline_sleep as *const () as usize),
        ("strata_host_now", strata_host_now as *const () as usize),
        ("strata_host_sleep", strata_host_sleep as *const () as usize),
        ("strata_cancelled", strata_cancelled as *const () as usize),
        ("strata_panic", strata_panic as *const () as usize),
        ("strata_was_unwound", strata_was_unwound as *const () as usize),
        ("strata_unwound", strata_unwound as *const () as usize),
        ("strata_scope_cancel", strata_scope_cancel as *const () as usize),
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
        ("strata_int_and", strata_int_and as *const () as usize),
        ("strata_int_or", strata_int_or as *const () as usize),
        ("strata_int_xor", strata_int_xor as *const () as usize),
        ("strata_shl", strata_shl as *const () as usize),
        ("strata_shr", strata_shr as *const () as usize),
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
        ("strata_str_ord", strata_str_ord as *const () as usize),
        ("strata_str_byte_len", strata_str_byte_len as *const () as usize),
        ("strata_str_byte_at", strata_str_byte_at as *const () as usize),
        ("strata_str_byte_slice", strata_str_byte_slice as *const () as usize),
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
    let _one = one_run();
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

/// `--trace`'s table (`llvm::Traced`), set when traced code is compiled.
static TRACED: std::sync::Mutex<Option<super::Traced>> = std::sync::Mutex::new(None);

pub fn set_traced(t: super::Traced) {
    *TRACED.lock().unwrap_or_else(|e| e.into_inner()) = Some(t);
}

thread_local! {
    /// The calls traced so far that haven't returned, and the arguments of
    /// the one being entered.
    static TRACE: std::cell::RefCell<(usize, Vec<String>)> = const { std::cell::RefCell::new((0, Vec::new())) };
}

/// A traced value, read by its shape, as the code that writes it.
fn traced_value(id: i64, which: Option<i64>, raw: i64) -> String {
    let t = TRACED.lock().unwrap_or_else(|e| e.into_inner());
    let Some(t) = t.as_ref() else { return "?".into() };
    let Some((_, args, ret)) = t.fns.get(id as usize) else { return "?".into() };
    let shape = match which {
        Some(k) => args.get(k as usize).and_then(|s| s.as_ref()),
        None => ret.as_ref(),
    };
    match shape {
        // SAFETY: the traced code passes a live value of the shape it was compiled with
        Some(s) => match unsafe { super::native::read(s, &t.named, raw) } {
            Ok(v) => crate::printer::literal(&v),
            Err(m) => format!("<{m}>"),
        },
        None => "()".into(),
    }
}

fn traced_name(id: i64) -> String {
    TRACED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|t| t.fns.get(id as usize)).map(|f| f.0.clone()).unwrap_or_default()
}

/// `strata_trace_arg(id, k, slot)`: argument `k` of the traced call about to start.
extern "C" fn trace_arg(id: i64, k: i64, raw: i64) {
    let v = traced_value(id, Some(k), raw);
    TRACE.with(|t| t.borrow_mut().1.push(v));
}

/// `strata_trace_call(id)`: a call, on stderr, indented by depth:
/// `→ total([Line { .. }])`.
extern "C" fn trace_call(id: i64) {
    let name = traced_name(id);
    TRACE.with(|t| {
        let mut t = t.borrow_mut();
        let args = std::mem::take(&mut t.1).join(", ");
        eprintln!("{}→ {name}({args})", "  ".repeat(t.0));
        t.0 += 1;
    });
}

/// `strata_trace_return(id, slot)`: its return and value: `← total: 2100`.
extern "C" fn trace_return(id: i64, raw: i64) {
    let name = traced_name(id);
    let has = TRACED.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|t| t.fns.get(id as usize)).is_some_and(|f| f.2.is_some());
    let v = if has { format!(": {}", traced_value(id, None, raw)) } else { String::new() };
    TRACE.with(|t| {
        let mut t = t.borrow_mut();
        t.0 = t.0.saturating_sub(1);
        eprintln!("{}← {name}{v}", "  ".repeat(t.0));
    });
}

/// The shapes of each fuzz driver's arguments, by the driver's address.
static FUZZ_SHAPES: std::sync::Mutex<Option<(std::collections::HashMap<usize, Vec<super::Shape>>, super::Shapes)>> = std::sync::Mutex::new(None);

thread_local! {
    /// The driver running (its address), and the arguments it made so far.
    static FUZZING: std::cell::RefCell<(usize, Vec<Expr>)> = const { std::cell::RefCell::new((0, Vec::new())) };
}

/// `strata_fuzz_keep(k, slot)`: the running driver's argument `k`, just
/// made, read by its shape (before the call it is passed to can change it).
extern "C" fn fuzz_keep(k: i64, raw: i64) {
    let shapes = FUZZ_SHAPES.lock().unwrap_or_else(|e| e.into_inner());
    let Some((per, named)) = shapes.as_ref() else { return };
    FUZZING.with(|f| {
        let mut f = f.borrow_mut();
        let Some(shape) = per.get(&f.0).and_then(|ss| ss.get(k as usize)) else { return };
        // SAFETY: the driver passes a live value of the shape it was compiled with
        let v = unsafe { super::native::read(shape, named, raw) }.unwrap_or_else(|m| Expr::Str(format!("<{m}>")));
        f.1.push(v);
    });
}

/// The fuzz drivers of IR `compile_fuzz` made, compiled for good (the
/// engine lives as long as the process): their addresses, in order. `shapes`:
/// each one's arguments'.
pub fn fuzzers(ir: &str, shapes: Vec<Vec<super::Shape>>, named: super::Shapes) -> Result<Vec<usize>, String> {
    use std::ffi::c_char;
    let ctx: &'static Context = Box::leak(Box::new(Context::create()));
    // unoptimized: compiling the drivers costs more than optimizing saves on a few hundred cases
    let engine = engine_at(ctx, ir, false)?;
    let addrs = (0..shapes.len()).map(|i| engine.get_function_address(&format!("strata.fuzz.{i}")).map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
    std::mem::forget(engine);
    *FUZZ_SHAPES.lock().unwrap_or_else(|e| e.into_inner()) = Some((addrs.iter().copied().zip(shapes).collect(), named));
    type Init = unsafe extern "C" fn(i32, *const *const c_char);
    let init: Init = unsafe { std::mem::transmute(strata_init as *const ()) };
    let name = std::ffi::CString::new("verify").unwrap();
    let argv = [name.as_ptr()];
    let _one = one_run();
    unsafe { init(1, argv.as_ptr()) };
    Ok(addrs)
}

/// One fuzzing case of the driver at `addr`, seeded, within `budget` steps:
/// the fault that ended it (kind, message, call stack), if any, and its
/// arguments, as the code that writes them.
pub fn fuzz_case(addr: usize, seed: u64, budget: i64) -> (Option<(String, String, String)>, Vec<Expr>) {
    let _one = one_run();
    use std::ffi::{c_char, CStr};
    type Fuzz = unsafe extern "C" fn(unsafe extern "C" fn(), i64, i64) -> i32;
    type Text = unsafe extern "C" fn() -> *const c_char;
    type Message = unsafe extern "C" fn() -> *const i64;
    let (fuzz, kind, message, stack): (Fuzz, Text, Message, Text) = unsafe {
        (
            std::mem::transmute(strata_fuzz as *const ()),
            std::mem::transmute(strata_test_kind as *const ()),
            std::mem::transmute(strata_test_message as *const ()),
            std::mem::transmute(strata_test_stack as *const ()),
        )
    };
    let text = |p: *const c_char| unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string();
    let f: unsafe extern "C" fn() = unsafe { std::mem::transmute(addr) };
    FUZZING.with(|f| *f.borrow_mut() = (addr, vec![]));
    let failed = unsafe { fuzz(f, seed as i64, budget) } != 0;
    let fault = failed.then(|| {
        let m = unsafe { message() };
        let msg = unsafe { std::slice::from_raw_parts(m.add(1) as *const u8, *m as usize) };
        (text(unsafe { kind() }), String::from_utf8_lossy(msg).to_string(), text(unsafe { stack() }))
    });
    (fault, FUZZING.with(|f| std::mem::take(&mut f.borrow_mut().1)))
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
/// The runtime's state is the process's (its heap, the budget, the message,
/// what was printed, the values kept): code is run one at a time, so threads
/// (the tests', the viewer's) don't run into each other's.
fn one_run() -> std::sync::MutexGuard<'static, ()> {
    static RUNTIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    RUNTIME.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn comptime(ir: &str, roots: &[super::Shape], shapes: &super::Shapes, budget: i64) -> Result<Vec<Result<crate::ast::Expr, Fault>>, String> {
    Ok(run_kept(ir, roots, shapes, budget, false)?.into_iter().map(|(r, _)| r).collect())
}

/// `comptime`, optionally optimized (`strata run f [args]`: what it runs may
/// be long), with what each one printed.
pub fn run_kept(ir: &str, roots: &[super::Shape], shapes: &super::Shapes, budget: i64, optimize: bool) -> Result<Vec<(Result<crate::ast::Expr, Fault>, String)>, String> {
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
    let _one = one_run();
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

/// Functions compiled once and called many times (`llvm::compile_calls`:
/// `strata serve`), with values the host keeps between calls: compiled
/// code's raw slots.
pub struct Calls<'ctx> {
    entries: Vec<unsafe extern "C" fn()>,
    _engine: inkwell::execution_engine::ExecutionEngine<'ctx>,
}

/// `f` given the calls in `ir`, compiled (optimized) and the runtime started.
pub fn with_calls<R>(ir: &str, n: usize, f: impl FnOnce(&Calls) -> R) -> Result<R, String> {
    let ctx = Context::create();
    let engine = engine_at(&ctx, ir, true)?;
    let mut entries = vec![];
    for i in 0..n {
        let addr = engine.get_function_address(&format!("strata.ct.{i}")).map_err(|e| e.to_string())?;
        entries.push(unsafe { std::mem::transmute::<usize, unsafe extern "C" fn()>(addr) });
    }
    let name = std::ffi::CString::new("serve").unwrap();
    let argv = [name.as_ptr()];
    {
        let _one = one_run();
        let init: unsafe extern "C" fn(i32, *const *const std::ffi::c_char) = unsafe { std::mem::transmute(strata_init as *const ()) };
        unsafe { init(1, argv.as_ptr()) };
    }
    Ok(f(&Calls { entries, _engine: engine }))
}

impl Calls<'_> {
    /// Call `i` with the arguments' slots (those it is given become its):
    /// the slot of its result, now the host's, or its fault; and what it
    /// printed.
    pub fn call(&self, i: usize, args: &[i64]) -> (Result<i64, Fault>, String) {
        use std::ffi::{c_char, CStr};
        type Fuzz = unsafe extern "C" fn(unsafe extern "C" fn(), i64, i64) -> i32;
        type Message = unsafe extern "C" fn() -> *const i64;
        type Text = unsafe extern "C" fn() -> *const c_char;
        let (fuzz, message, kind, stack, printed): (Fuzz, Message, Text, Text, Text) = unsafe {
            (
                std::mem::transmute(strata_fuzz as *const ()),
                std::mem::transmute(strata_test_message as *const ()),
                std::mem::transmute(strata_test_kind as *const ()),
                std::mem::transmute(strata_test_stack as *const ()),
                std::mem::transmute(strata_test_printed as *const ()),
            )
        };
        let text = |p: *const c_char| unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string();
        // (not under `one_run`: a call may need strata to run compiled code,
        // `Codebase`'s snapshot verifying, which it does on a thread of its own)
        unsafe { strata_set_given(args.as_ptr()) };
        // (no steps are counted: the budget is never reached)
        let r = if unsafe { fuzz(self.entries[i], 1, i64::MAX) } != 0 {
            let m = unsafe { message() };
            let msg = unsafe { std::slice::from_raw_parts(m.add(1) as *const u8, *m as usize) };
            Err(Fault { kind: text(unsafe { kind() }), message: String::from_utf8_lossy(msg).to_string(), stack: text(unsafe { stack() }) })
        } else {
            Ok(unsafe { strata_ct_kept() })
        };
        unsafe { strata_set_given(std::ptr::null()) };
        (r, text(unsafe { printed() }))
    }
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
        let resolved = super::building_std(|| crate::resolve::resolve(std).program);
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

/// `Code.version()` for compiled code (std.code): the codebase's fingerprint.
extern "C" fn code_version() -> i64 {
    super::native::new_string(&crate::ui::code_version(std::path::Path::new(&crate::ui::code_root())))
}

/// `Code.snapshot()` for compiled code: the snapshot (src/ui.rs) read against
/// `std.code.Snapshot` and written into compiled memory, or 0 and the
/// failure in `*err`.
extern "C" fn code_snapshot(err: *mut i64) -> i64 {
    let fail = |m: String| {
        unsafe { *err = super::native::new_string(&m) };
        0
    };
    // the shape of a snapshot: std's, the same in every program (tags come from names)
    static SHAPE: std::sync::OnceLock<Result<(super::Shape, super::Shapes), String>> = std::sync::OnceLock::new();
    let shape = SHAPE.get_or_init(|| {
        let t = crate::ast::TypeExpr::Named("std.code.Snapshot".into(), vec![]);
        let (mut roots, shapes) = super::shapes_of(crate::stdlib::program(), &[t])?;
        Ok((roots.pop().ok_or("no shape for `std.code.Snapshot`")?, shapes))
    });
    let (root, shapes) = match shape {
        Ok(s) => s,
        Err(e) => return fail(e.clone()),
    };
    // made on a thread of its own: making it runs compiled code (verifying
    // refinements), which would reset this thread's run (its trap, what it printed)
    let made = std::thread::Builder::new()
        .stack_size(1 << 30)
        .spawn(|| crate::ui::code_snapshot(std::path::Path::new(&crate::ui::code_root())))
        .map_err(|e| e.to_string())
        .and_then(|t| t.join().map_err(|_| "making the snapshot panicked".to_string()))
        .and_then(|r| r);
    let e = match made {
        Ok(e) => e,
        Err(e) => return fail(e),
    };
    match super::native::write(root, shapes, &e) {
        Ok(p) => p,
        Err(e) => fail(format!("the codebase's snapshot doesn't fit `std.code.Snapshot`: {e}")),
    }
}

fn engine_at<'ctx>(ctx: &'ctx Context, ir: &str, optimize: bool) -> Result<inkwell::execution_engine::ExecutionEngine<'ctx>, String> {
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
    // the runtime's data the compiled code reads
    if let Some(g) = module.get_global("strata_cancel_any") {
        engine.add_global_mapping(&g, &raw const strata_cancel_any as usize);
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
