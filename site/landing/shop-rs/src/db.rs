use std::collections::HashMap;

use crate::model::{Order, OrderId};

/// Storage for orders. `save` adds or replaces one order and keeps the rest.
pub trait Db {
    fn load(&self, id: &OrderId) -> Option<Order>;
    fn save(&mut self, id: &OrderId, o: Order);
}

/// Orders in memory.
#[derive(Default)]
pub struct MemDb {
    pub all: HashMap<OrderId, Order>,
}

impl Db for MemDb {
    fn load(&self, id: &OrderId) -> Option<Order> {
        self.all.get(id).cloned()
    }

    fn save(&mut self, id: &OrderId, o: Order) {
        self.all.insert(id.clone(), o);
    }
}
