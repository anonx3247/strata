# std.sqlite

SQLite databases, through the system's libsqlite3.

A database is reached through capabilities: `main` (or a test) asks for
`Sqlite` and opens a file, or a database in memory, and hands the rest of
the program the `Connection` as a `Database`, so tests can pass an
in-memory database or a `mock`:

```rust
fn main(sqlite: Sqlite) -> Result[(), SqlError] {
  mut db := sqlite.open("todo.db")!
  (mut db.run("CREATE TABLE IF NOT EXISTS todo (title TEXT NOT NULL)"))!
  _ := (mut db.execute("INSERT INTO todo VALUES (?)", ["write docs"]))!
  for row in db.query("SELECT title FROM todo", [])! {
    print(row.text(0)!)
  }
  mut db.close()
}
```

Every failure is a value (`SqlError`): SQLite's own errors, a column read
as the wrong type, an integer beyond 64 bits. Values cross as `SqlValue`:
`Int`, `Float`, `String`, `Bool` (stored as 0 or 1) and `Null`; an
`Option` becomes one with `sql(x)`. Blobs don't cross yet (SQL's `hex`
and `unhex` carry them as text).

## Types

### Null

```rust
type Null
```

The type of SQL's NULL, as a value of its own.

### SqlValue

```rust
type SqlValue = Int | Float | String | Bool | Null
```

A value crossing to SQLite or back: SQLite's storage classes but blobs,
and `Bool` (stored as the integer 0 or 1, read back as an `Int`).

### SqlError

```rust
type SqlError =
  /// The database couldn't be opened (a missing folder, no permission).
  | CantOpen { path: String, message: String }
  /// A constraint refused the change (`UNIQUE`, `NOT NULL`, `CHECK`, a
  /// foreign key).
  | Constraint { message: String }
  /// Another connection holds the database (SQLITE_BUSY, SQLITE_LOCKED).
  | Busy { message: String }
  /// Column `column` holds a value of another type than the one asked for.
  | WrongType { column: Int, wanted: String, found: String }
  /// There is no column `column`: the rows have `count`.
  | NoColumn { column: Int, count: Int }
  /// There is no parameter `index`: the statement has `count`.
  | NoParameter { index: Int, count: Int }
  /// `given` values for a statement with `wanted` parameters.
  | WrongParameters { given: Int, wanted: Int }
  /// A value SQLite can't hold or this module can't read yet: an integer
  /// beyond 64 bits, a NUL character in text, a blob.
  | NotRepresentable { message: String }
  /// Any other error SQLite reported: its result code and message.
  | SqlFailed { code: Int, message: String }
  /// A row read back that its record type refuses (`std.orm`): a value
  /// breaking a refinement.
  | BadRow { table: String, message: String }
```

Why a database operation failed.

### Sqlite

```rust
capability Sqlite {
  /// The database in the file `path`, created if missing.
  fn open(path: String) -> Result[Connection, SqlError]
  /// A new, empty database in memory, gone once closed.
  fn memory() -> Result[Connection, SqlError]
  /// The version of the SQLite library, `3.51.0`.
  fn version() -> String
}
```

SQLite itself. Only the runtime makes one, for `main` and tests that ask
for it.

### Row

```rust
type Row = { values: List[SqlValue] }
```

A row of a query's result: its values, by column.

### Connection

```rust
type Connection = { handle: Sqlite3, path: String }
```

A database SQLite opened (`sqlite.open(path)`). Close it when done.

### Statement

```rust
type Statement = { handle: Sqlite3Stmt, sql: String }
```

A prepared statement (`db.prepare(sql)`): bind its parameters, step
through its rows, read their columns. Finalize it when done.

## Methods

### Row.value

```rust
fn value(self, i: Int) -> Result[SqlValue, SqlError]
```

The value of column `i` (from 0).

### Row.int

```rust
fn int(self, i: Int) -> Result[Int, SqlError]
```

Column `i` as an integer.

### Row.float

```rust
fn float(self, i: Int) -> Result[Float, SqlError]
```

Column `i` as a float (an integer column too).

### Row.text

```rust
fn text(self, i: Int) -> Result[String, SqlError]
```

Column `i` as text.

### Row.bool

```rust
fn bool(self, i: Int) -> Result[Bool, SqlError]
```

Column `i` as a boolean: an integer, 0 for `false`.

### Row.is_null

```rust
fn is_null(self, i: Int) -> Result[Bool, SqlError]
```

Whether column `i` is NULL.

### Connection.prepare

```rust
fn prepare(self, sql: String) -> Result[Statement, SqlError]
```

`sql` (one statement) compiled, its parameters to bind.

### Connection.close

```rust
fn close(mut self) -> Result[(), SqlError]
```

Close the connection (what it prepared must be finalized first).

### Connection.autocommit

```rust
fn autocommit(self) -> Bool
```

Whether no transaction is open.

### Statement.parameter_count

```rust
fn parameter_count(self) -> Int where 0 <= it && it <= 2147483647
```

How many parameters (`?`) the statement has.

### Statement.column_count

```rust
fn column_count(self) -> Int where 0 <= it && it <= 2147483647
```

How many columns its rows have.

### Statement.column_name

```rust
fn column_name(self, i: Int) -> Result[String, SqlError]
```

The name of column `i` (from 0).

### Statement.bind

```rust
fn bind(mut self, i: Int, v: SqlValue) -> Result[(), SqlError]
```

Bind parameter `i` (from 1, as SQLite counts them) to `v`.

### Statement.bind_all

```rust
fn bind_all(mut self, params: List[SqlValue]) -> Result[(), SqlError]
```

Bind every parameter, in order: as many values as it has parameters.

### Statement.step

```rust
fn step(mut self) -> Result[Bool, SqlError]
```

Run the statement to its next row: `true` when there is one to read,
`false` once it is done.

### Statement.value

```rust
fn value(self, i: Int) -> Result[SqlValue, SqlError]
```

Column `i` of the current row.

### Statement.row

```rust
fn row(self) -> Result[Row, SqlError]
```

The current row, every column.

### Statement.reset

```rust
fn reset(mut self) -> Result[(), SqlError]
```

Back to before the first row, the bindings kept.

### Statement.finalize

```rust
fn finalize(mut self)
```

Free the statement: it can't be used after.

## Traits

### Sql

```rust
trait Sql {
  /// This value as SQLite stores it.
  fn to_sql(self) -> SqlValue
}
```

Values that cross to SQLite as an `SqlValue`.

### Database

```rust
capability trait Database {
  /// Run one statement with its parameters (`?` in `sql`, in order): how
  /// many rows it changed.
  fn execute(
    mut self,
    sql: String,
    params: List[SqlValue],
  ) -> Result[Int, SqlError]
  /// Run one statement with its parameters: the rows it gives.
  fn query(
    self,
    sql: String,
    params: List[SqlValue],
  ) -> Result[List[Row], SqlError]
  /// Run statements without parameters, one after the other (a schema).
  fn run(mut self, sql: String) -> Result[(), SqlError]
  /// The row id of the last row inserted.
  fn last_insert_id(self) -> Int
  /// Start a transaction.
  fn begin(mut self) -> Result[(), SqlError]
  /// Keep what the transaction changed.
  fn commit(mut self) -> Result[(), SqlError]
  /// Undo what the transaction changed.
  fn rollback(mut self) -> Result[(), SqlError]
}
```

Running SQL: a database SQLite opened (a `Connection`), or a stand-in for
it.

## Functions

### sql

```rust
fn sql[T: Sql](x: T) -> SqlValue
```

`x` as an `SqlValue`: an `Option` is its value or `Null`.

## Implementations

- `impl Sql for Int`
- `impl Sql for Float`
- `impl Sql for String`
- `impl Sql for Bool`
- `impl[T: Sql] Sql for Option[T]`
- `impl Database for Connection`
