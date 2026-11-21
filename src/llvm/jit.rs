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
    fn strata_init();
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
    fn strata_mod();
    fn strata_fmod();
    fn strata_finite();
    fn strata_show_int();
    fn strata_show_float();
    fn strata_show_bool();
    fn strata_json_str();
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
        ("strata_host_read_line", strata_host_read_line as *const () as usize),
        ("strata_host_write", strata_host_write as *const () as usize),
        ("strata_pid", strata_pid as *const () as usize),
        ("strata_init", strata_init as *const () as usize),
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
        ("strata_mod", strata_mod as *const () as usize),
        ("strata_fmod", strata_fmod as *const () as usize),
        ("strata_finite", strata_finite as *const () as usize),
        ("strata_show_int", strata_show_int as *const () as usize),
        ("strata_show_float", strata_show_float as *const () as usize),
        ("strata_show_bool", strata_show_bool as *const () as usize),
        ("strata_json_str", strata_json_str as *const () as usize),
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

/// Run the IR's `main` with the program's arguments; its exit code. A fault
/// exits the process from the runtime, as a compiled program does.
pub fn run(ir: &str, program: &str, args: &[String]) -> Result<i32, String> {
    Target::initialize_native(&InitializationConfig::default())?;
    let ctx = Context::create();
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
    module.run_passes("default<O2>", &machine, PassBuilderOptions::create()).map_err(|e| e.to_string())?;
    let engine = module.create_jit_execution_engine(OptimizationLevel::Aggressive).map_err(|e| e.to_string())?;
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
    let main = unsafe { engine.get_function::<unsafe extern "C" fn(i32, *const *const std::ffi::c_char) -> i32>("main") }.map_err(|e| e.to_string())?;
    // argv as C has it: the program's name, then its arguments
    let owned: Vec<std::ffi::CString> = std::iter::once(program).chain(args.iter().map(|a| a.as_str())).map(|a| std::ffi::CString::new(a).unwrap_or_default()).collect();
    let argv: Vec<*const std::ffi::c_char> = owned.iter().map(|a| a.as_ptr()).collect();
    Ok(unsafe { main.call(argv.len() as i32, argv.as_ptr()) })
}
