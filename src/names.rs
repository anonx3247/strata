//! Qualified names: `shop.orders.Order` is the definition `Order` of the
//! module `shop.orders`. The root module's definitions have no prefix.

/// The last part of a qualified name: `Order` for `shop.orders.Order`.
pub fn short(q: &str) -> &str {
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
