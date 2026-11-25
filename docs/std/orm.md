# std.orm

Records as SQLite tables. `derive Table for Todo` gives the table's schema, its rows written and read back as `Todo`s, and typed queries over its fields.

## Types

### Column

```rust
type Column = {
  name: String,
  /// `INTEGER`, `REAL` or `TEXT`.
  sql_type: String,
  /// Whether it may be NULL: an `Option` field.
  nullable: Bool,
  /// Whether it is the table's key, the field `id`.
  key: Bool,
  /// The table whose keys its values are (`REFERENCES`).
  references: Option[String],
  /// Its field's refinement as written: `it != ""`.
  condition: Option[String],
  /// Its field's default as written: `false`. It is the `DEFAULT` when it is a literal.
  default: Option[String] = None,
}
```

A column of a table, as `CREATE TABLE` declares it.

### Schema

```rust
type Schema[R] = {
  table: String,
  columns: List[Column],
  condition: Option[String],
}
```

A table of records `R`: its name, its columns and the record's own refinement as written (`it.start <= it.end`). Made by `Todo.schema()`.

### Cond

```rust
type Cond[R] = { sql: String, params: List[SqlValue] }
```

A condition on rows of the table of `R`, such as `Todo.done.eq(false)`: SQL with `?` parameters, and their values in order.
Values are always passed as parameters, never as SQL text.

### Query

```rust
type Query[R] = {
  cond: Option[Cond[R]],
  /// `"column" ASC` or `"column" DESC`, the first given first.
  order: List[String],
  limit: Option[Int],
}
```

A query on the table of `R`: the rows meeting a condition, in an order, at most so many.
Made with `Todo.query()` or `Todo.filter(c)`. Run with `all`, `first`, `count` or `delete`.

## Methods

### Schema.sql

```rust
fn sql(self) -> String
```

`CREATE TABLE IF NOT EXISTS ...`, with a `CHECK` for each refinement SQL can say.

### Schema.create

```rust
fn create(self, mut db: Database) -> Result[(), SqlError]
```

Creates the table if it is missing, and turns on SQLite's `REFERENCES` checks on this connection.

### Cond.and

```rust
fn and(self, other: Cond[R]) -> Cond[R]
```

Rows meeting both.

### Cond.either

```rust
fn either(self, other: Cond[R]) -> Cond[R]
```

Rows meeting either.

### Cond.not

```rust
fn not(self) -> Cond[R]
```

Rows not meeting it.

### Query.filter

```rust
fn filter(self, c: Cond[R]) -> Query[R]
```

Narrows to rows that also meet `c`.

### Query.order_by

```rust
fn order_by[T](self, col: KeyPath[R, T]) -> Query[R]
```

Orders by `col`, smallest first, after the orders given before.

### Query.order_by_desc

```rust
fn order_by_desc[T](self, col: KeyPath[R, T]) -> Query[R]
```

Orders by `col`, largest first.

### Query.limit

```rust
fn limit(self, n: Natural) -> Query[R]
```

At most `n` rows.

### Query.clauses

```rust
fn clauses(self) -> (String, List[SqlValue])
```

What follows `FROM table`: the condition, the order and the limit, with their parameters.

## Traits

### Compare

```rust
trait Compare[R, T] {
  /// Rows whose column is `v`.
  fn eq(self, v: T) -> Cond[R]
  /// Rows whose column isn't `v`.
  fn ne(self, v: T) -> Cond[R]
  /// Rows whose column is less than `v`.
  fn lt(self, v: T) -> Cond[R]
  /// Rows whose column is at most `v`.
  fn le(self, v: T) -> Cond[R]
  /// Rows whose column is more than `v`.
  fn gt(self, v: T) -> Cond[R]
  /// Rows whose column is at least `v`.
  fn ge(self, v: T) -> Cond[R]
}
```

Compares a column with a value of its field's type: `Todo.done.eq(false)`, where `Todo.done` is a `KeyPath[Todo, Bool]`.
`eq` and `ne` match `None` too (SQL's `IS`). An order with `None` holds for no row.

### Rows

```rust
trait Rows[R] {
  /// Every row the query selects.
  fn all(self, db: Database) -> Result[List[R], SqlError]
  /// The first row it selects, if any.
  fn first(self, db: Database) -> Result[Option[R], SqlError]
  /// How many rows it selects.
  fn count(self, db: Database) -> Result[Int, SqlError]
  /// Deletes the rows it selects. Returns how many.
  fn delete(self, mut db: Database) -> Result[Int, SqlError]
}
```

Runs a query, giving its rows as records.

### Table

```rust
derivable(Record) trait Table {
  /// The table: its name, a column per field in order, and the record's own refinement.
  derived fn schema() -> Schema[Self]
  /// Its values, one per column, in order.
  derived fn values(self) -> List[SqlValue]
  /// The record a row holds, columns in field order, every refinement checked. A row breaking one is `BadRow`.
  derived fn from_row(r: Row) -> Result[Self, SqlError]
  /// The record with the key SQLite gave it. A key that isn't an integer is kept.
  derived fn with_key(self, id: Int) -> Self
  /// Adds the record as a new row and returns it as kept.
  /// An integer key is the one SQLite gives, and the `id` given is ignored.
  fn insert(self, mut db: Database) -> Result[Self, SqlError]
  /// Writes the record over the row with its key. Returns whether there was one.
  fn update(self, mut db: Database) -> Result[Bool, SqlError]
  /// Deletes the row with the record's key. Returns whether there was one.
  fn delete(self, mut db: Database) -> Result[Bool, SqlError]
  /// The row whose key is `key`, if there is one.
  fn get[K: Sql](db: Database, key: K) -> Result[Option[Self], SqlError]
  /// Every row, in the order SQLite keeps them.
  fn all(db: Database) -> Result[List[Self], SqlError]
  /// A query of every row, to narrow with `filter`, order and limit.
  fn query() -> Query[Self]
  /// A query of the rows meeting `c`.
  fn filter(c: Cond[Self]) -> Query[Self]
}
```

A record kept as a row of a table: `derive Table for Todo`.

- The table is named after the type in snake case: `todo_item` for `TodoItem`.
- Each field is a column. `Int` and `Bool` (0 or 1) are `INTEGER`, `Float` is `REAL`, `String` is `TEXT`. An `Option` drops `NOT NULL`. A `distinct` type is what it is made of.
- The field `id` is the `PRIMARY KEY`. An integer key is SQLite's row id, given to each row `insert` adds.
- A `distinct` field whose type is named `<Table>Id` (`project: ProjectId`) is a key of that table: `REFERENCES project (id)`.
- A refinement is a `CHECK` where SQL can say it, and a literal default is a `DEFAULT`.
- Every row read back is checked again in Strata. A row breaking a refinement SQL couldn't state is a `BadRow` error.

## Implementations

- `impl[R, T: Sql] Compare[R, T] for KeyPath[R, T]`
- `impl[R: Table] Rows[R] for Query[R]`
