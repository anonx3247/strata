from std.sqlite use Sqlite, SqlError, Sql, SqlValue

from std.orm use Table, Compare, Rows, Key

from std.json use Json

distinct type ProjectId = Int

distinct type TodoId = Int

impl Sql for TodoId {
  fn to_sql(self) -> SqlValue {
    self.value
  }
}

type Project = { id: ProjectId = ProjectId(0), name: String where it != "" && !contains(it, "/") }

derive Table for Project

type Todo = {
  id: TodoId = TodoId(0),
  project: Key[Project],
  title: String where it != "" && byte_len(it) <= 80,
  priority: Int where 1 <= it && it <= 5,
  due: Option[Int] = None,
  done: Bool = false,
}

derive Table for Todo

fn demo(sqlite: Sqlite) -> Result[(), SqlError] {
  mut db := sqlite.memory()!
  Project.schema().create(mut db)!
  Todo.schema().create(mut db)!
  print(Todo.schema().sql())
  home := (Project { name: "home" }).insert(mut db)!
  print(home)
  a := (Todo { project: home.key(), title: "write docs", priority: 2 }).insert(mut db)!
  b := (Todo { project: home.key(), title: "buy milk", priority: 1, due: Some(3) }).insert(mut db)!
  _ := (Todo { project: home.key(), title: "fix bike", priority: 4 }).insert(mut db)!
  print(a)
  print(Todo.get(db, b.id)!)
  print(a.project.load(db))
  print(Todo.filter(Todo.done.eq(false)).order_by(Todo.priority).all(db)!.map(t -> t.title))
  print((Todo { ..a, done: true }).update(mut db))
  print(Todo.filter(Todo.done.eq(false).and(Todo.priority.ge(2))).count(db))
  print(Todo.query().order_by_desc(Todo.title).limit(2).all(db)!.map(t -> t.title))
  print(Todo.filter(Todo.due.eq(None)).count(db))
  print(b.delete(mut db))
  print(Todo.all(db)!.map(t -> t.title))
  // a to-do of a project that isn't there: SQLite refuses it
  print((Todo { project: Key { value: 7 }, title: "lost", priority: 3 }).insert(mut db))
  // a project a to-do names: SQLite refuses to delete it
  print(home.delete(mut db))
  // a row SQL's CHECK refuses
  print(mut db.execute("INSERT INTO todo (project, title, priority) VALUES (1, '', 3)", []))
  // a row read back that breaks a refinement no CHECK states
  (mut db.run("CREATE TABLE weird (id INTEGER PRIMARY KEY, name TEXT NOT NULL)"))!
  print(mut db.execute("INSERT INTO project (name) VALUES ('')", []))
  _ := (mut db.execute("INSERT INTO project (name) VALUES ('a/b')", []))!
  print(Project.all(db))
  print(Project.schema().sql())
  print(Todo.filter(Todo.title.eq("fix bike")).delete(mut db))
  print(Todo.all(db)!.map(t -> t.title))
  Ok(())
}

/// A span of time with a note: its own refinement a table CHECK.
type Span = {
  id: Int = 0,
  start: Float,
  end: Float,
  note: Option[String] = None,
}
  where it.start <= it.end

derive Table for Span

/// A tag, keyed by its text.
type Tag = { id: String where it != "", color: String = "red" }

derive Table for Tag

/// An item, keyed by its code (a field other than `id`).
type Item = { code: String where it != "", name: String }

derive Table for Item {
  key code
}

/// A place on a shelf: its key two columns.
type Slot = { shelf: Int, slot: Int, item: Key[Item], note: Option[Key[Tag]] = None }

derive Table for Slot {
  key (shelf, slot)
}

/// Where something is: kept as JSON.
type Address = { city: String where it != "", zip: String }

derive Json for Address

/// A delivery, with a record and a list kept as JSONB.
type Delivery = { id: Int = 0, to: Address, items: List[String] = [] }

derive Table for Delivery

fn more(sqlite: Sqlite) -> Result[(), SqlError] {
  mut db := sqlite.memory()!
  Span.schema().create(mut db)!
  Tag.schema().create(mut db)!
  Item.schema().create(mut db)!
  Slot.schema().create(mut db)!
  Delivery.schema().create(mut db)!
  print(Span.schema().sql())
  print(Tag.schema().sql())
  print(Item.schema().sql())
  print(Slot.schema().sql())
  print(Delivery.schema().sql())
  s := (Span { start: 1.5, end: 2.0, note: Some("lunch") }).insert(mut db)!
  print(s)
  print(mut db.execute("INSERT INTO span (start, \"end\") VALUES (3.0, 1.0)", []))
  print((Tag { id: "home" }).insert(mut db))
  print((Tag { id: "home" }).insert(mut db))
  print(Tag.get(db, "home"))
  print(Tag.get(db, "work"))
  print(Span.filter(Span.note.ne(None)).all(db))
  bolt := (Item { code: "B1", name: "bolt" }).insert(mut db)!
  print(bolt)
  print((Item { code: "B1", name: "another" }).insert(mut db))
  print((Slot { shelf: 1, slot: 2, item: bolt.key() }).insert(mut db))
  print((Slot { shelf: 1, slot: 3, item: bolt.key(), note: Some(Key { value: "home" }) }).insert(mut db))
  print((Slot { shelf: 1, slot: 2, item: bolt.key() }).insert(mut db))
  print((Slot { shelf: 1, slot: 4, item: Key { value: "X9" } }).insert(mut db))
  print((Slot { shelf: 1, slot: 4, item: bolt.key(), note: Some(Key { value: "work" }) }).insert(mut db))
  print((Slot { shelf: 1, slot: 2, item: bolt.key() }).delete(mut db))
  print(Slot.all(db))
  d := (Delivery { to: Address { city: "Paris", zip: "75001" }, items: ["tea", "cups"] }).insert(mut db)!
  print(Delivery.all(db))
  print(Delivery.get(db, d.id))
  // JSON the record refuses, written by hand
  _ := (mut db.execute("INSERT INTO delivery (\"to\", items) VALUES (jsonb('{\"city\": \"\", \"zip\": \"1\"}'), jsonb('[]'))", []))!
  print(Delivery.all(db))
  Ok(())
}

fn main(sqlite: Sqlite) {
  print(demo(sqlite))
  print(more(sqlite))
}
