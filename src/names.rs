//! Qualified names: `shop.orders.Order` is the definition `Order` of the
//! module `shop.orders`. The root module's definitions have no prefix.

/// The last part of a qualified name: `Order` for `shop.orders.Order`.
pub fn short(q: &str) -> &str {
    // (a root function named like a builtin, as resolved: `main$get` is `get`)
    let q = q.strip_prefix(crate::resolve::ROOT_FN).unwrap_or(q);
    q.rsplit('.').next().unwrap_or(q)
}

/// The module part of a qualified name: `shop.orders` for
/// `shop.orders.Order`, `""` for a name of the root module.
pub fn module_of(q: &str) -> &str {
    q.rsplit_once('.').map_or("", |(m, _)| m)
}

/// `module.name`, or `name` in the root module.
pub fn qualify(module: &str, name: &str) -> String {
    if module.is_empty() { name.to_string() } else { format!("{module}.{name}") }
}

/// `name` in module `module`, as a symbol: `orders.Line`, or `Line` in the root module.
pub fn qualified(module: crate::symbol::Symbol, name: crate::symbol::Symbol) -> crate::symbol::Symbol {
    if module.is_empty() { name } else { format!("{module}.{name}").into() }
}

/// A call stack as faults show it, outermost first: four or more calls in a
/// row of one function (a recursion) are one entry, `down (95846 times)`.
pub fn show_stack(stack: &[String]) -> String {
    fold_stack(stack).join(" -> ")
}

/// A call stack's entries, a recursion's calls as one (`show_stack`).
pub fn fold_stack(stack: &[String]) -> Vec<String> {
    let mut parts = vec![];
    let mut i = 0;
    while i < stack.len() {
        let n = stack[i..].iter().take_while(|f| **f == stack[i]).count();
        if n >= 4 {
            parts.push(format!("{} ({n} times)", stack[i]));
        } else {
            parts.extend(std::iter::repeat_n(stack[i].clone(), n));
        }
        i += n;
    }
    parts
}
