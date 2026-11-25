from std.sqlite use Sqlite, Database, Connection, SqlError, Null, sql

from std.fs use Disk

/// A table, rows in it (an `Option` as `Null`), a query back.
fn basics(mut db: Database) -> Result[(), SqlError] {
  (mut db.run(
    "CREATE TABLE item (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, price REAL, qty INTEGER, note TEXT, fresh INTEGER)",
  ))!
  let note: Option[String] = None
  print(mut db.execute(
    "INSERT INTO item (name, price, qty, note, fresh) VALUES (?, ?, ?, ?, ?)",
    ["apple", 1.25, 3, sql(note), true],
  ))
  print(db.last_insert_id())
  print(mut db.execute(
    "INSERT INTO item (name, price, qty, note, fresh) VALUES (?, ?, ?, ?, ?), (?, ?, ?, ?, ?)",
    [
      "pear",
      0.5,
      -9223372036854775808,
      sql(Some("ripe")),
      false,
      "fig",
      Null,
      9223372036854775807,
      "é ✓",
      Null,
    ],
  ))
  for r in db.query("SELECT id, name, price, qty, note, fresh FROM item ORDER BY id", [])! {
    print(r)
    print((r.int(0), r.text(1), r.float(2), r.int(3), r.is_null(4), r.bool(5)))
  }
  print(db.query("SELECT name FROM item WHERE price > ? ORDER BY name", [0.75]))
  Ok(())
}

/// A transaction rolled back, then one committed.
fn transactions(mut db: Database) -> Result[(), SqlError] {
  (mut db.begin())!
  print(mut db.execute("DELETE FROM item", []))
  print(db.query("SELECT count(*) FROM item", []))
  (mut db.rollback())!
  print(db.query("SELECT count(*) FROM item", []))
  (mut db.begin())!
  print(mut db.execute("UPDATE item SET qty = qty + 1 WHERE name = ?", ["apple"]))
  (mut db.commit())!
  print(db.query("SELECT qty FROM item WHERE name = 'apple'", []))
  Ok(())
}

/// Failures, each a value.
fn errors(mut db: Database) {
  print(mut db.execute("INSERT INTO item (name) VALUES (?)", ["apple"]))
  print(mut db.execute("INSERT INTO item (price) VALUES (?)", [1.0]))
  print(db.query("SELECT nope FROM item", []))
  print(db.query("SELEC 1", []))
  print(db.query("SELECT ?", []))
  print(db.query("SELECT ?", [1, 2]))
  print(db.query("SELECT ?", [9223372036854775808]))
  print(db.query("SELECT ?", ["a" + chr(0) + "b"]))
  print(db.query("SELECT x'00ff'", []))
  print(mut db.commit())
  print(mut db.run("CREATE TABLE item (x)"))
  match db.query("SELECT name, qty FROM item WHERE name = 'fig'", []) {
    Ok([r]) => print((r.text(1), r.int(1), r.int(2), r.float(0), r.bool(0))),
    other => print(other),
  }
}

/// A prepared statement: bound, stepped, reset, bound again.
fn prepared(db: Connection) -> Result[(), SqlError] {
  mut s := db.prepare("SELECT ? * 2 AS twice, upper(?) AS up")!
  print((s.parameter_count(), s.column_count(), s.column_name(0), s.column_name(1), s.column_name(2)))
  (mut s.bind(1, 21))!
  (mut s.bind(2, "abc"))!
  print(mut s.step())
  print((s.value(0), s.value(1), s.row()))
  print(mut s.step())
  (mut s.reset())!
  (mut s.bind(1, 2.5))!
  print(mut s.step())
  print(s.row())
  print(mut s.bind(3, 1))
  mut s.finalize()
  print(db.prepare(""))
  Ok(())
}

/// A database in a file outlives its connection.
fn in_a_file(sqlite: Sqlite, mut disk: Disk) -> Result[(), SqlError] {
  _ := mut disk.remove("items.db")
  mut db := sqlite.open("items.db")!
  (mut db.run("CREATE TABLE t (x TEXT); INSERT INTO t VALUES ('kept')"))!
  print(db.autocommit())
  (mut db.close())!
  mut again := sqlite.open("items.db")!
  print(again.query("SELECT x FROM t", []))
  (mut again.close())!
  print(disk.exists("items.db"))
  _ := mut disk.remove("items.db")
  print(sqlite.open("no/such/folder/items.db"))
  Ok(())
}

fn main(sqlite: Sqlite, mut disk: Disk) -> Result[(), SqlError] {
  print(sqlite.version() != "")
  mut db := sqlite.memory()!
  (basics(mut db))!
  (transactions(mut db))!
  errors(mut db)
  (prepared(db))!
  (mut db.close())!
  in_a_file(sqlite, mut disk)
}

/// How many items a database holds.
fn count(db: Database) -> Result[Int, SqlError] {
  match db.query("SELECT count(*) FROM item", [])! {
    [r] => r.int(0),
    _ => Ok(0),
  }
}

/// A database in memory with two items.
fn two_items(sqlite: Sqlite) -> Result[Int, SqlError] {
  mut db := sqlite.memory()!
  (mut db.run("CREATE TABLE item (name TEXT)"))!
  _ := (mut db.execute("INSERT INTO item VALUES (?), (?)", ["a", "b"]))!
  n := count(db)!
  (mut db.close())!
  Ok(n)
}

test "a test asks for SQLite" (sqlite: Sqlite) {
  assert two_items(sqlite) == Ok(2)
}

test "a mock database" {
  db := mock Database
  r := count(db)
  assert r is Ok || r is Err
}
