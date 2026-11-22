//! Running independent work on all cores.

/// `f` of each item, on as many threads as there are cores (fewer for few
/// items), in the items' order.
pub fn map<T: Sync, R: Send + Sync>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: Vec<std::sync::OnceLock<R>> = (0..items.len()).map(|_| std::sync::OnceLock::new()).collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(items.len()).max(1);
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(x) = items.get(k) else { break };
                let _ = out[k].set(f(x));
            });
        }
    });
    out.into_iter().map(|r| r.into_inner().expect("every item was done")).collect()
}
