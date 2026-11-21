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
    fn strata_str_eq();
    fn strata_str_len();
    fn strata_fault();
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
}

fn runtime() -> Vec<(&'static str, usize)> {
    vec![
        ("strata_concat", strata_concat as *const () as usize),
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
        ("strata_str_eq", strata_str_eq as *const () as usize),
        ("strata_str_len", strata_str_len as *const () as usize),
        ("strata_fault", strata_fault as *const () as usize),
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
    ]
}

/// Run the IR's `main`; its exit code. A fault exits the process from the
/// runtime, as a compiled program does.
pub fn run(ir: &str) -> Result<i32, String> {
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
    for (name, addr) in runtime() {
        if let Some(f) = module.get_function(name) {
            engine.add_global_mapping(&f, addr);
        }
    }
    let main = unsafe { engine.get_function::<unsafe extern "C" fn() -> i32>("main") }.map_err(|e| e.to_string())?;
    Ok(unsafe { main.call() })
}
