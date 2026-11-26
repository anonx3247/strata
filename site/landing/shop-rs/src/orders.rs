use crate::db::Db;
use crate::model::{Cents, Line, Order, OrderId, ShipError};

pub fn line_total(l: &Line) -> Cents {
    l.qty * l.unit_price
}

pub fn total(lines: &[Line]) -> Cents {
    lines.iter().map(line_total).sum()
}

/// Never more than `amount`.
pub fn discount(amount: Cents) -> Cents {
    if amount >= 10000 {
        amount - amount / 10
    } else {
        amount
    }
}

/// Ships a pending order. Any other order is left as it is.
pub fn ship(db: &mut impl Db, id: &OrderId, tracking: String) -> Result<Order, ShipError> {
    let order = db.load(id).ok_or(ShipError::NotFound)?;
    match order {
        Order::Pending { lines, .. } => {
            let o = Order::Shipped { id: id.clone(), lines, tracking };
            db.save(id, o.clone());
            Ok(o)
        }
        _ => Err(ShipError::NotPending),
    }
}

/// Cancels a pending order: a shipped one can't be.
pub fn cancel(db: &mut impl Db, id: &OrderId, reason: String) -> Result<Order, ShipError> {
    let order = db.load(id).ok_or(ShipError::NotFound)?;
    match order {
        Order::Pending { .. } => {
            let o = Order::Cancelled { id: id.clone(), reason };
            db.save(id, o.clone());
            Ok(o)
        }
        _ => Err(ShipError::NotPending),
    }
}

/// What to refund for an order. Never negative.
pub fn refund(o: &Order) -> Cents {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(sku: &str, qty: i64, unit_price: Cents) -> Line {
        Line { sku: sku.into(), qty, unit_price }
    }

    #[test]
    fn totals() {
        let lines = vec![line("a", 3, 100), line("b", 1, 50)];
        assert_eq!(total(&lines), 350);
        assert_eq!(discount(20000), 18000);
    }
}
