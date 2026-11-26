use std::collections::HashMap;

/// Not enough money in the account.
#[derive(Debug, PartialEq)]
pub struct Insufficient {
    pub short_by: u64,
}

/// Accounts holding money, in cents.
///
/// Rules every implementation must follow (nothing checks them):
/// - a transfer never makes or loses money
/// - a transfer that fails changes nothing
pub trait Ledger {
    /// What an account holds (0 for one never used).
    fn balance(&self, id: u64) -> u64;

    /// Moves money from one account to another, if there is enough.
    fn transfer(&mut self, from: u64, to: u64, amount: u64) -> Result<(), Insufficient>;
}

/// Accounts in memory.
#[derive(Default)]
pub struct MemLedger {
    pub accounts: HashMap<u64, u64>,
}

impl Ledger for MemLedger {
    fn balance(&self, id: u64) -> u64 {
        *self.accounts.get(&id).unwrap_or(&0)
    }

    fn transfer(&mut self, from: u64, to: u64, amount: u64) -> Result<(), Insufficient> {
        let have = self.balance(from);
        if amount > have {
            return Err(Insufficient { short_by: amount - have });
        }
        let gets = self.balance(to);
        self.accounts.insert(from, have - amount);
        self.accounts.insert(to, gets + amount);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> MemLedger {
        MemLedger { accounts: HashMap::from([(1, 100), (2, 50)]) }
    }

    #[test]
    fn transfer_moves_the_money() {
        let mut l = ledger();
        assert_eq!(l.transfer(1, 2, 30), Ok(()));
        assert_eq!((l.balance(1), l.balance(2)), (70, 80));
    }

    #[test]
    fn transfer_keeps_the_total() {
        let mut l = ledger();
        l.transfer(1, 2, 40).unwrap();
        assert_eq!(l.balance(1) + l.balance(2), 150);
    }

    #[test]
    fn failed_transfer_changes_nothing() {
        let mut l = ledger();
        assert_eq!(l.transfer(2, 1, 80), Err(Insufficient { short_by: 30 }));
        assert_eq!((l.balance(1), l.balance(2)), (100, 50));
    }
}
