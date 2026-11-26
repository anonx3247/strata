/// Order identifiers: `ord_` followed by 6 lowercase letters or digits.
pub type OrderId = String;

/// Money in cents. Never negative.
pub type Cents = i64;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub sku: String,
    /// Between 1 and 99.
    pub qty: i64,
    pub unit_price: Cents,
}

/// An order's life: placed, then shipped or cancelled, never both.
#[derive(Clone, Debug, PartialEq)]
pub enum Order {
    Pending { id: OrderId, lines: Vec<Line> },
    Shipped { id: OrderId, lines: Vec<Line>, tracking: String },
    Cancelled { id: OrderId, reason: String },
}

#[derive(Debug, PartialEq)]
pub enum ShipError {
    NotFound,
    NotPending,
}
