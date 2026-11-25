# std.sqlite

std.sqlite: SQLite databases through the system's libsqlite3.
`main` asks for `Sqlite` and passes the `Connection` on as a `Database`, so tests can use an in-memory database or a mock.

## Types

### Null

```rust
type Null
```

SQL's NULL, as a type of its own.

### SqlValue

```rust
type SqlValue = Int | Float | String | Bool | Null
```

A value crossing to SQLite or back: SQLite's storage classes, plus `Bool`.
A `Bool` is stored as 0 or 1 and read back as an `Int`.
Not supported: blobs. SQL's `hex` and `unhex` pass them as text.

### SqlError

```rust
type SqlError =
  /// The database couldn't be opened: a missing folder, no permission.
  | CantOpen { path: String, message: String }
  /// A constraint refused the change: `UNIQUE`, `NOT NULL`, `CHECK`, a foreign key.
  | Constraint { message: String }
  /// Another connection holds the database (SQLITE_BUSY, SQLITE_LOCKED).
  | Busy { message: String }
  /// Column `column` holds another type than the one asked for.
  | WrongType { column: Int, wanted: String, found: String }
  /// There is no column `column`. The rows have `count`.
  | NoColumn { column: Int, count: Int }
  /// There is no parameter `index`. The statement has `count`.
  | NoParameter { index: Int, count: Int }
  /// `given` values for a statement with `wanted` parameters.
  | WrongParameters { given: Int, wanted: Int }
  /// A value SQLite can't hold or this module can't read: an integer beyond 64 bits, a NUL in text, a blob.
  | NotRepresentable { message: String }
  /// Any other SQLite error, with its result code and message.
  | SqlFailed { code: Int, message: String }
  /// A row read back that its record type refuses (`std.orm`): a value breaks a refinement.
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
  /// The SQLite library's version, such as `3.51.0`.
  fn version() -> String
}
```

SQLite itself. Only the runtime makes one, for `main` and tests that ask for it.

### Row

```rust
type Row = { values: List[SqlValue] }
```

A result row's values, by column.

### Connection

```rust
type Connection = { handle: Sqlite3, path: String }
```

A database SQLite opened with `sqlite.open(path)`. Close it when done.

### Statement

```rust
type Statement = { handle: Sqlite3Stmt, sql: String }
```

A prepared statement from `db.prepare(sql)`. Bind its parameters, step through its rows, read their columns. Finalize it when done.

## Methods

### Row.value

```rust
fn value(self, i: Int) -> Result[SqlValue, SqlError]
```

The value of column `i`, from 0.

### Row.int

```rust
fn int(self, i: Int) -> Result[Int, SqlError]
```

Column `i` as an integer.

### Row.float

```rust
fn float(self, i: Int) -> Result[Float, SqlError]
```

Column `i` as a float. An integer column converts.

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

Compiles one statement.

### Connection.close

```rust
fn close(mut self) -> Result[(), SqlError]
```

Closes the connection. What it prepared must be finalized first.

### Connection.autocommit

```rust
fn autocommit(self) -> Bool
```

Whether no transaction is open.

### Statement.parameter_count

```rust
fn parameter_count(self) -> Int where 0 <= it && it <= 2147483647
```

How many `?` parameters the statement has.

### Statement.column_count

```rust
fn column_count(self) -> Int where 0 <= it && it <= 2147483647
```

How many columns its rows have.

### Statement.column_name

```rust
fn column_name(self, i: Int) -> Result[String, SqlError]
```

The name of column `i`, from 0.

### Statement.bind

```rust
fn bind(mut self, i: Int, v: SqlValue) -> Result[(), SqlError]
```

Binds parameter `i` to `v`. Parameters count from 1, as in SQLite.

### Statement.bind_all

```rust
fn bind_all(mut self, params: List[SqlValue]) -> Result[(), SqlError]
```

Binds every parameter in order. `params` must have one value per parameter.

### Statement.step

```rust
fn step(mut self) -> Result[Bool, SqlError]
```

Steps to the next row. `true` when there is one to read, `false` once done.

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

Goes back to before the first row, keeping the bindings.

### Statement.finalize

```rust
fn finalize(mut self)
```

Frees the statement. It can't be used after.

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
  /// Runs one statement with its `?` parameters in order. Returns how many rows it changed.
  fn execute(
    mut self,
    sql: String,
    params: List[SqlValue],
  ) -> Result[Int, SqlError]
  /// Runs one statement with its parameters. Returns its rows.
  fn query(
    self,
    sql: String,
    params: List[SqlValue],
  ) -> Result[List[Row], SqlError]
  /// Runs statements without parameters one after the other, such as a schema.
  fn run(mut self, sql: String) -> Result[(), SqlError]
  /// The row id of the last row inserted.
  fn last_insert_id(self) -> Int
  /// Starts a transaction.
  fn begin(mut self) -> Result[(), SqlError]
  /// Keeps what the transaction changed.
  fn commit(mut self) -> Result[(), SqlError]
  /// Undoes what the transaction changed.
  fn rollback(mut self) -> Result[(), SqlError]
}
```

Runs SQL: a `Connection`, or a stand-in in tests.

## Functions

### sql

```rust
fn sql[T: Sql](x: T) -> SqlValue
```

`x` as an `SqlValue`. An `Option` gives its value or `Null`.

## Implementations

- `impl Sql for Int`
- `impl Sql for Float`
- `impl Sql for String`
- `impl Sql for Bool`
- `impl[T: Sql] Sql for Option[T]`
- `impl Database for Connection`
