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
        let hashes = time(&mut || p.decls.iter().for_each(|d| drop(std::hint::black_box((crate::store::self_hash(d), crate::store::interface_hash(d), crate::store::code_hash(d))))));
        println!("{name:<15} parse {parse:>9.1?}  decode {decode:>9.1?}  clone {clone:>9.1?}  resolve {resolve:>9.1?}  graph {graph:>9.1?}  check {check:>9.1?}  hashes {hashes:>9.1?}");
    }
}
