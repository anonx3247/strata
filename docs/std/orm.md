# std.orm

Records as SQLite tables: `derive Table for Todo` gives the
table's schema, its rows written and read back as `Todo`s, and typed
queries over its fields.

```rust
type Project = { id: Int = 0, name: String where it != "" }

type Todo = {
  id: Int = 0,
  title: String where it != "",
  priority: Int where 1 <= it && it <= 5,
  done: Bool = false,
  project: Key[Project],
}

derive Table for Project
derive Table for Todo

fn main(sqlite: Sqlite) -> Result[(), SqlError] {
  mut db := sqlite.memory()!
  Project.schema().create(mut db)!
  Todo.schema().create(mut db)!
  p := (Project { name: "home" }).insert(mut db)!
  t := (Todo { title: "write docs", priority: 2, project: p.key() })
    .insert(mut db)!
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
The key is written in the derive's block, `derive Table for Item { key
code }` (`key (shelf, slot)` for one of several columns); without one,
the field `id`, which an integer one SQLite gives each row (its row id).
A `Key[T]` field is a key of `T`'s table (`REFERENCES`), an
`Option[Key[T]]` one that may be NULL; a field holding a record, a union,
a list or a dict whose type derives `Json` is kept as JSONB. A
refinement is a `CHECK` where SQL can say it (comparisons of `it`,
`byte_len(it)`, `it.field` for the record's own, literals, `&&`, `||`,
`!`; of conditions joined by `&&`, those it can), a literal default is a
`DEFAULT`, and every row read back is checked again in Strata, so a row
breaking a refinement SQL couldn't state is an error value (`BadRow`),
never a fault.

The keys are invariants `verify` checks, over the database's model
(`Database.tables`, each table's keys): `insert` requires the rows its
`Key[T]` fields name to exist and its key to be new (unless SQLite gives
it), `update` the same of the rows it names, and each says how the model
changes; a delete of a row another names is refused by SQLite, an error
value.

Queries are values: `Todo.title` is a `KeyPath[Todo, String]`, so
`Todo.title.eq("x")` (a `Cond[Todo]`) is checked against the field's
type, and runs with the value as a parameter (`"title" IS ?`), never as
SQL text.

## Types

### Column

```rust
type Column = {
  name: String,
  /// `INTEGER`, `REAL`, `TEXT`, or `BLOB` (JSONB).
  sql_type: String,
  /// Whether it may be NULL (an `Option` field).
  nullable: Bool,
  /// Whether it is the table's key, or one of its columns.
  key: Bool,
  /// The table and column its values are keys of (a `Key[T]` field):
  /// `REFERENCES`.
  references: Option[(String, String)],
  /// Its field's refinement, as written (`it != ""`).
  condition: Option[String],
  /// Its field's default, as written (`false`): its `DEFAULT` when it is a
  /// literal.
  default: Option[String] = None,
  /// Whether SQLite gives its values: the default key `id`, an integer (the
  /// row id).
  generated: Bool = false,
  /// Whether it holds JSONB (a record, union, list or dict deriving
  /// `Json`): written with `jsonb(?)`, read with `json(..)`.
  json: Bool = false,
}
```

A column of a table, as `CREATE TABLE` declares it.

### Schema

```rust
type Schema[R] = {
  table: String,
  columns: List[Column],
  condition: Option[String],
  key: List[String] = [],
}
```

A table of records `R`: its name, its columns, the record's own
refinement (`it.start <= it.end`), as written, and its key's columns.
`Todo.schema()`.

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

### Key

```rust
type Key[T] = { value: KeyValue }
```

A key of a row of `T`'s table (a key of one column: an integer or text).
A field `project: Key[Project]` is a foreign key: its column holds a
`Project`'s key and `REFERENCES` its table, and `verify` checks that the
row exists when a record holding it is inserted or updated. `p.key()`
gives a row's; `k.load(db)` reads the row.

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
  assumed ensures db.rows == old db.rows
```

Make the table if it is missing (and have SQLite check `REFERENCES`
on this connection): no row changes.

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

### Key.load

```rust
fn load(self, db: Database) -> Result[Option[T], SqlError]
  assumed ensures match it {
    Ok(Some(r)) => contains(db.rows, (r.table_name(), self.value)),
    _ => true,
  }
```

The row of `T` with this key, if there is one.

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
  /// field's each, in order), the record's own refinement and its key.
  derived fn schema() -> Schema[Self]
  /// The name of its table (the type's, in snake case).
  derived fn table_name(self) -> String
  /// Its values, a column's each, in order (a JSON one as text).
  derived fn values(self) -> Result[List[SqlValue], SqlError]
  /// Its key (of several columns: their values written out, as
  /// `key_value_of` writes them).
  derived fn key(self) -> Key[Self]
    assumed ensures it == self.key()
  /// Whether each row its `Key[T]` fields name is in `rows` (a
  /// database's model).
  derived fn references_held(self, rows: Set[(String, KeyValue)]) -> Bool
  /// Whether its key is new in `rows` (always, when SQLite gives it; a
  /// key of several columns isn't followed: SQLite refuses it again).
  derived fn key_free(self, rows: Set[(String, KeyValue)]) -> Bool
  /// The record a row holds (its columns in field order), every
  /// refinement checked: a row breaking one is `BadRow`.
  derived fn from_row(r: Row) -> Result[Self, SqlError]
  /// The record with the key SQLite gave it (the default key `id`, an
  /// integer; others are kept).
  derived fn with_key(self, id: Int) -> Self
  /// Add the record as a new row: the record as kept, with the key SQLite
  /// gave it when it gives one (the `id` given is then left out). The rows
  /// its `Key[T]` fields name must exist, and its key be new.
  fn insert(self, mut db: Database) -> Result[Self, SqlError]
    assumed requires self.references_held(db.rows)
    assumed requires self.key_free(db.rows)
    assumed ensures match it {
      Ok(r) => db.rows == with(old db.rows, (self.table_name(), r.key().value)),
      Err(_) => db.rows == old db.rows,
    }
  /// Write the record over the row with its key: whether there was one.
  /// The rows its `Key[T]` fields name must exist.
  fn update(self, mut db: Database) -> Result[Bool, SqlError]
    assumed requires self.references_held(db.rows)
    assumed ensures db.rows == old db.rows
  /// Delete the row with the record's key: whether there was one. A row
  /// another table's `Key[T]` names isn't deleted: an error.
  fn delete(self, mut db: Database) -> Result[Bool, SqlError]
    assumed ensures match it {
      Ok(true) => db.rows == without(old db.rows, (self.table_name(), self.key().value)),
      _ => db.rows == old db.rows,
    }
  /// The row whose key is `key` (a key of one column), if there is one.
  fn get[K: Sql](db: Database, key: K) -> Result[Option[Self], SqlError]
  /// Every row, in the order SQLite keeps them.
  fn all(db: Database) -> Result[List[Self], SqlError]
  /// A query of every row, to narrow (`filter`), order and limit.
  fn query() -> Query[Self]
  /// A query of the rows meeting `c`.
  fn filter(c: Cond[Self]) -> Query[Self]
}
```

A record kept as a row of a table (see above): `derive Table for Todo`,
`derive Table for Item { key code }` (see above).

## Implementations

- `impl[R, T: Sql] Compare[R, T] for KeyPath[R, T]`
- `impl[R: Table] Rows[R] for Query[R]`
- `impl[T] Sql for Key[T]`
