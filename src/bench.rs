//! Timings of the front end on the examples: `cargo test --release bench --
//! --ignored --nocapture`. Not a test: numbers to compare before and after a
//! change.

#[test]
#[ignore]
fn bench() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in ["examples/tasks", "std"] {
        let p = crate::package::program(&root.join(name)).unwrap();
        // the same modules as text, to compare parsing with decoding
        let mut texts: Vec<(String, String)> = Vec::new();
        for d in &p.decls {
            if !texts.iter().any(|(m, _)| *m == d.module) {
                texts.push((d.module.to_string(), crate::printer::module_file(&p, &d.module)));
            }
        }
        let modules: Vec<Vec<u8>> = crate::binary::encode_modules(&p).into_iter().map(|(_, b)| b).collect();
        // the best of 10 batches of 20 runs: other load on the machine only slows a batch down
        let time = |f: &mut dyn FnMut()| {
            (0..10)
                .map(|_| {
                    let t = std::time::Instant::now();
                    for _ in 0..20 {
                        f()
                    }
                    t.elapsed() / 20
                })
                .min()
                .unwrap()
        };
        let parse = time(&mut || drop(std::hint::black_box(texts.iter().map(|(m, t)| crate::parser::parse_module_file(t, m).unwrap()).collect::<Vec<_>>())));
        let decode = time(&mut || modules.iter().for_each(|m| drop(std::hint::black_box(crate::binary::decode_module(m).unwrap()))));
        let clone = time(&mut || drop(std::hint::black_box(p.clone())));
        let resolve = time(&mut || drop(std::hint::black_box(crate::resolve::resolve(&p))));
        let graph = time(&mut || drop(std::hint::black_box(crate::store::graph(&p))));
        let check = time(&mut || drop(std::hint::black_box(crate::types::check(&p))));
        let once = time(&mut || drop(std::hint::black_box(crate::types::check_and_elaborate(&p))));
        let c = crate::types::check_and_elaborate(&p);
        let refinements = time(&mut || drop(std::hint::black_box(crate::verify::refinements_in(&c.program, &c.compiled, &c.env))));
        let full = time(&mut || drop(std::hint::black_box(crate::full_check(&p))));
        let load = time(&mut || drop(std::hint::black_box(crate::package::program(&root.join(name)).unwrap())));
        println!("{name:<15} load {load:>9.1?}  check_and_elaborate {once:>9.1?}  refinements {refinements:>9.1?}  full_check {full:>9.1?}");
        let hashes = time(&mut || p.decls.iter().for_each(|d| drop(std::hint::black_box((crate::store::self_hash(d), crate::store::interface_hash(d), crate::store::code_hash(d))))));
        println!("{name:<15} parse {parse:>9.1?}  decode {decode:>9.1?}  clone {clone:>9.1?}  resolve {resolve:>9.1?}  graph {graph:>9.1?}  check {check:>9.1?}  hashes {hashes:>9.1?}");
    }
}

/// The type checker in a loop, for a profiler to sample:
/// `STRATA_PROFILE=examples/tasks cargo test --release profile_check -- --ignored`.
#[test]
#[ignore]
fn profile_check() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let name = std::env::var("STRATA_PROFILE").unwrap_or("examples/tasks".into());
    let p = crate::package::program(&root.join(name)).unwrap();
    let t = std::time::Instant::now();
    let mut n = 0;
    while t.elapsed().as_secs() < 8 {
        std::hint::black_box(crate::types::check_and_elaborate(&p));
        n += 1;
    }
    println!("{n} checks, {:?} each", t.elapsed() / n);
}

/// Counts allocations in test builds: a measure of work that machine load
/// doesn't blur.
struct Counting;
static ALLOCS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::System.realloc(p, l, n) }
    }
}
#[global_allocator]
static COUNTING: Counting = Counting;

/// Allocations of one check of each example: `cargo test --release allocations -- --ignored --nocapture`.
#[test]
#[ignore]
fn allocations() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for name in ["examples/tasks", "std"] {
        let p = crate::package::program(&root.join(name)).unwrap();
        let count = |f: &dyn Fn()| {
            let before = ALLOCS.load(std::sync::atomic::Ordering::Relaxed);
            f();
            ALLOCS.load(std::sync::atomic::Ordering::Relaxed) - before
        };
        let resolve = count(&|| drop(crate::resolve::resolve(&p)));
        let check = count(&|| drop(crate::types::check_and_elaborate(&p)));
        let full = count(&|| drop(crate::full_check(&p)));
        println!("{name:<15} allocations: resolve {resolve}  check_and_elaborate {check}  full_check {full}");
    }
}
