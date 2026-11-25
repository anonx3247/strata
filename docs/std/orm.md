# std.orm

Records as SQLite tables: `derive Table for Todo` gives the
table's schema, its rows written and read back as `Todo`s, and typed
queries over its fields.

```rust
distinct type TodoId = Int

type Todo = {
  id: TodoId = TodoId(0),
  title: String where it != "",
  priority: Int where 1 <= it && it <= 5,
  done: Bool = false,
}

derive Table for Todo

fn main(sqlite: Sqlite) -> Result[(), SqlError] {
  mut db := sqlite.memory()!
  Todo.create_table(mut db)!
  t := (Todo { title: "write docs", priority: 2 }).insert(mut db)!
  open := Todo.filter(Todo.done.eq(false))
    .order_by(Todo.priority)
    .all(db)!
  ..
}
```

The table is named after the type in snake case (`TodoItem`:
`todo_item`), a column per field: `Int` and `Bool` (0 or 1) are
`INTEGER`, `Float` is `REAL`, `String` is `TEXT`, an `Option` is the
same column without `NOT NULL`, a `distinct` type is what it is made of.
The field named `id` is the key (`PRIMARY KEY`): an integer one is
SQLite's row id, given to each row `insert` adds; another `distinct` id
field (`list: ListId`) is a key of the table its type names (`REFERENCES
list (id)`). A refinement is a `CHECK` where SQL can say it (comparisons
of `it`, `byte_len(it)`, `it.field` for the record's own, literals,
`&&`, `||`, `!`), and every row read back is checked again in Strata, so
a row breaking a refinement SQL couldn't state is an error value
(`BadRow`), never a fault.

Queries are values: `Todo.title` is a `KeyPath[Todo, String]`, so
`Todo.title.eq("x")` (a `Cond[Todo]`) is checked against the field's
type, and runs with the value as a parameter (`"title" IS ?`), never as
SQL text.

## Types

### Column

```rust
type Column = {
  name: String,
  /// `INTEGER`, `REAL` or `TEXT`.
  sql_type: String,
  /// Whether it may be NULL (an `Option` field).
  nullable: Bool,
  /// Whether it is the table's key (the field `id`).
  key: Bool,
  /// The table whose key its values are (`REFERENCES`).
  references: Option[String],
  /// Its field's refinement, as written (`it != ""`).
  condition: Option[String],
  /// Its field's default, as written (`false`): its `DEFAULT` when it is a
  /// literal.
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

A table of records `R`: its name, its columns and the record's own
refinement (`it.start <= it.end`), as written. `Todo.schema()`.

### Cond

```rust
type Cond[R] = { sql: String, params: List[SqlValue] }
```

A condition on rows of the table of `R` (`Todo.done.eq(false)`): SQL
with `?` parameters and their values, in order.

### Query

```rust
type Query[R] = {
  cond: Option[Cond[R]],
  /// `"column" ASC` / `"column" DESC`, the first first.
  order: List[String],
  limit: Option[Int],
}
```

A query on the table of `R`: the rows meeting a condition, in an order,
at most so many. Made with `Todo.query()` or `Todo.filter(c)`, run with
`all`, `first`, `count` or `delete`.

## Methods

### Schema.sql

```rust
fn sql(self) -> String
```

`CREATE TABLE IF NOT EXISTS ...`: a `CHECK` for each refinement SQL
can say.

### Schema.create

```rust
fn create(self, mut db: Database) -> Result[(), SqlError]
```

Make the table if it is missing (and have SQLite check `REFERENCES`
on this connection).

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

Only the rows meeting `c` too.

### Query.order_by

```rust
fn order_by[T](self, col: KeyPath[R, T]) -> Query[R]
```

Ordered by the column `col`, smallest first (after the orders given
before).

### Query.order_by_desc

```rust
fn order_by_desc[T](self, col: KeyPath[R, T]) -> Query[R]
```

Ordered by the column `col`, largest first.

### Query.limit

```rust
fn limit(self, n: Natural) -> Query[R]
```

At most `n` rows.

### Query.clauses

```rust
fn clauses(self) -> (String, List[SqlValue])
```

What follows `FROM table`: the condition, the order and the limit,
with their parameters.

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

Comparing a column with a value of its field's type: `Todo.done.eq(false)`.
`eq` and `ne` match `None` too (SQL's `IS`); an order with `None` holds
for no row.

### Rows

```rust
trait Rows[R] {
  /// Every row the query selects.
  fn all(self, db: Database) -> Result[List[R], SqlError]
  /// The first row it selects, if any.
  fn first(self, db: Database) -> Result[Option[R], SqlError]
  /// How many rows it selects.
  fn count(self, db: Database) -> Result[Int, SqlError]
  /// Delete the rows it selects: how many.
  fn delete(self, mut db: Database) -> Result[Int, SqlError]
}
```

Running queries: the rows they select, as records.

### Table

```rust
derivable(Record) trait Table {
  /// The table: its name (the type's, in snake case), its columns (a
  /// field's each, in order) and the record's own refinement.
  derived fn schema() -> Schema[Self]
  /// Its values, a column's each, in order.
  derived fn values(self) -> List[SqlValue]
  /// The record a row holds (its columns in field order), every
  /// refinement checked: a row breaking one is `BadRow`.
  derived fn from_row(r: Row) -> Result[Self, SqlError]
  /// The record with the key SQLite gave it (an integer key; others are
  /// kept).
  derived fn with_key(self, id: Int) -> Self
  /// Add the record as a new row: the record as kept, with the key SQLite
  /// gave it when the key is an integer (the `id` given is then left out).
  fn insert(self, mut db: Database) -> Result[Self, SqlError]
  /// Write the record over the row with its key: whether there was one.
  fn update(self, mut db: Database) -> Result[Bool, SqlError]
  /// Delete the row with the record's key: whether there was one.
  fn delete(self, mut db: Database) -> Result[Bool, SqlError]
  /// The row whose key is `key`, if there is one.
  fn get[K: Sql](db: Database, key: K) -> Result[Option[Self], SqlError]
  /// Every row, in the order SQLite keeps them.
  fn all(db: Database) -> Result[List[Self], SqlError]
  /// A query of every row, to narrow (`filter`), order and limit.
  fn query() -> Query[Self]
  /// A query of the rows meeting `c`.
  fn filter(c: Cond[Self]) -> Query[Self]
}
```

A record kept as a row of a table (see above): `derive Table for Todo`.

## Implementations

- `impl[R, T: Sql] Compare[R, T] for KeyPath[R, T]`
- `impl[R: Table] Rows[R] for Query[R]`
