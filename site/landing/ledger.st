/// Money in cents, never negative.
type Cents = Int
  where it >= 0

/// An account number.
type Account = Int
  where it >= 0

/// Not enough money in the account.
type Insufficient = { short_by: Cents }

/// Accounts holding money.
trait Ledger {
  model balances: Dict[Account, Cents]

  /// What an account holds (0 for one never used).
  fn balance(self, id: Account) -> Cents
    ensures it == (self.balances.get(id) ?? 0)

  /// Moves money from one account to another, if there is enough.
  fn transfer(mut self, from: Account, to: Account, amount: Cents) -> Result[(), Insufficient]
    ensures it is Ok == (amount <= old (self.balances.get(from) ?? 0))
    ensures it is Err ==> self.balances == old self.balances

  /// A transfer moves money: it never makes or loses any.
  law transfer_keeps_the_total(l: Self, from: Account, to: Account, amount: Cents) {
    mut r := l
    before := r.balance(from) + r.balance(to)
    _ := mut r.transfer(from, to, amount)
    r.balance(from) + r.balance(to) == before
  }

  /// A transfer that fails changes nothing.
  law failed_transfer_changes_nothing(l: Self, from: Account, to: Account, amount: Cents) {
    mut r := l
    ok := mut r.transfer(from, to, amount)
    ok is Ok || r.balance(from) == l.balance(from) && r.balance(to) == l.balance(to)
  }
}

/// Accounts in memory.
type MemLedger = { accounts: Dict[Account, Cents] }

impl Ledger for MemLedger {
  model balances = self.accounts

  fn balance(self, id: Account) -> Cents {
    self.accounts.get(id) ?? 0
  }

  fn transfer(mut self, from: Account, to: Account, amount: Cents) -> Result[(), Insufficient] {
    have := self.balance(from)
    if amount > have {
      return Err(Insufficient { short_by: amount - have })
    }
    gets := self.balance(to)
    mut self.accounts.insert(from, have - amount)
    mut self.accounts.insert(to, gets + amount)
    Ok(())
  }
}
