# The binary format

Strata code is stored as its syntax tree, in a binary format specific to Strata. A `.strata` file is either a **module file** (`main.strata`, `shop/orders.strata`) or a **bundle** holding a whole program. Text is a view of it.

- **Text goes in through an edit.** `strata edit` (from stdin or an editor) and `strata add FILE` parse text and store the result. Nothing else reads text, and a `.strata` file that holds text is refused.
- **Text comes out through `view`.** `strata view MODULE` prints a module in reading order (docs/tools.md, "Views"), `view --split DIR` writes one text file per module (`shop/orders.st`), and `view SELECTOR --level 0|1|2` shows definitions at a level of detail.
- `strata pack app.strata` writes a bundle, which `--file`, `run` and every view read like a package. `strata pack --prelude FILE` writes the prelude's definitions (its types and code, under the names programs use) as a module file, for Strata code that evaluates trees (`compiler/eval`).
- `strata header core.strata` prints a module file's table of contents: each definition's id, hashes and dependencies, then the ids of its variants and of what it refers to elsewhere. `--tree` prints each definition (of a bundle too) as a `compiler/ast` value, one per line. `--text` prints it as the printer writes it, as stored, with qualified references (`view` shortens them).
- In git, `strata setup-git` makes `git diff` and `git log -p` show `.strata` files as text, and merges them definition by definition. A conflict is left as text, and `strata resolve FILE` stores it again once fixed.

The encoder and decoder are hand-written, with no serialization library, in `compiler/binary`. They work on the tree defined in `compiler/ast`, and compute hashes with macOS's SHA-256 (CommonCrypto's `CC_SHA256`, through `compiler/c/cache.c`). Every module file in the repository round-trips through them (`cli-every_example_round_trips_through_render`). This page is the format's specification, kept in step with `compiler/binary`.

Design:

- **A header inside the module file.** Like a C header, it comes first and lists every definition: its id, name, kind, visibility, three hashes, its dependencies and its size in bytes. Listing a module, building the dependency graph or finding what changed reads only the header: 14 µs for all of `examples/tasks`, against about 170 µs to decode everything.
- **Interface first, bodies after.** Each definition's bytes start with its interface (types, signatures, contracts, laws), with each body written as `()`. The bodies follow. Checking code against a module needs only the interfaces, so a reader can stop there.
- **Definitions with their own strings.** Each definition carries its own strings. Its references are indices into its file's id table, so its bytes mean the same only next to that table.
- **References by id** (see [References by id](#references-by-id)). Every definition and variant has a stable id, a random 32-bit number given when it is first stored, unique within its module. A definition refers to another by its module and id, and the module file's header says which name each has. Renaming a definition changes one name in its module's header: no definition that uses it changes.
- **Qualified names in memory.** In memory, and in every pass of the compiler, a reference is its qualified name (`orders.Line`), however it was written (`Line` under `from orders use Line`, or `orders.Line` under `use orders`). A call to another module's function is a call of that name: `orders.ship(x)` is `Call(Var("orders.ship"), [x])`, not a method call on `orders`. Locals, fields, method calls (resolved from their receiver), and names that resolve to nothing are kept as written. When code is shown, each reference is written as short as the module's `use`s allow, so text is normalized: `orders.Line` written where `Line` is imported shows as `Line`. A module file shown alone (`git diff`) uses its own `use` lines. The conversions are `stored` and `written` in `compiler/resolve`; the encoder turns each qualified name into its id, and the decoder back.
- **Exact.** Decoding returns exactly the tree that was encoded, including comments, docs, blank lines between groups, big integers (as their digits) and floats (as their bits).
- **Canonical.** A tree has one encoding. Strings are numbered in the order they are first written, integers use the shortest form, and the decoder refuses anything else. A header's dependency lists can go stale when another module changes outside strata, or when an edit changes what an untouched definition would now depend on, because an edit writes only the files it changes and keeps each untouched definition's bytes and entry. `strata upgrade` rewrites every file.
- **Safe to read.** A truncated or corrupt file is an error, never a crash. Lengths can't exceed the rest of the file, nesting deeper than 10,000 levels is refused, a reference to an id the header doesn't name is refused, and a definition that isn't in the file's module, or doesn't match the kind and visibility in its header entry, is rejected. Hashes are computed when a definition is stored and trusted when it is read, so reading doesn't rehash. `binary.read_checked` also encodes each definition again in the named form and checks it against its hash, which catches any change inside a definition or to a name in the header (`compiler-bad_input`). The ids in the header aren't checked: a damaged one reads as another id (references find their names by index, so the tree reads the same but for that id).

A file of another format version is rejected. See [Changing the format](#changing-the-format).

## Files

```text
module-file = "STRATA" version:uint 0  header  definitions
bundle      = "STRATA" version:uint 1  manifest:option(raw-string)  modules:list(raw-bytes)
```

- A file starts with the six ASCII bytes `STRATA` (`53 54 52 41 54 41`), the format `version`, then `0` for a module file or `1` for a bundle. The version is `1`; while the format is unsettled, changes convert the files instead of bumping it.
- A bundle holds the package's `strata.toml` as written (none for a script), then each module file as its length and bytes, in the order the modules first appear in the program.

### The header

```text
header = strings:list(raw-string)  module:str  variants:list(Name)  refs:list(Ref)  entries:list(Entry)
Name   = id  name:str
Ref    = module:str  id  name:str
Entry  = id  name:str  kind:byte  public:bool  hash:32 bytes  interface_hash:32 bytes  code_hash:32 bytes
         deps:list(str)  size:uint  interface_size:uint
```

- The header's strings are its own table (names, the module path, dependencies). They are all text.
- `module` is the dotted module path (`""` for the root module).
- `variants`: the id and qualified name (`shop.Paid`) of each variant of the module's sums that is stored by id, in the order of the definitions.
- `refs`: the module, id and qualified name of each definition or variant of another module that the definitions refer to by id, in increasing order of module (by bytes), then id. The name is the one it had when the file was written: the module that has the id is what names it (see [References by id](#references-by-id)).
- The file's **id table** is the entries' keys in order (the file's module and each one's id), then the `variants`', then the `refs`'. A definition refers to a key by its index in this table. Each id is written once, here. A key given twice is refused.
- `id` is a definition's stable id.
- `name` is what `strata` calls the definition: `total`, `Order`, `test "totals"`, `impl Show for Order`, `use orders`. Its qualified name (`shop.total`, bare in the root module) is the name of its id.
- `kind` is the `DeclKind` tag below.
- `deps` are the qualified names of the definitions it refers to, sorted.
- The definitions follow the header back to back, in the order of the entries. A definition's offset is the end of the header plus the sizes of those before it, and the file ends exactly after the last one.

The three hashes are SHA-256:

| Hash | Of | Changes when |
|---|---|---|
| `hash` | the definition's named bytes | anything changes, a comment or a name it uses included |
| `interface_hash` | the interface part of the named bytes with every comment and doc removed | what other code relies on changes: types, signatures, contracts, laws, visibility, its module |
| `code_hash` | the named bytes with every comment and doc removed | what it does changes (verification results are keyed by it) |

The named bytes are the definition encoded with every reference as its name instead of its id (`binary.encode_definition`). So the hashes are of the content as text shows it, and the id is not part of them: two definitions with the same text have the same hashes, and a rename changes the hashes of the renamed definition and of each user, as their text changes, though the users' stored bytes don't. Caches keyed by hashes (verdicts, compile-time values, compiled code) see names, as they did before ids. Storing an entry encodes its definition twice: once named, for the hashes and to find the names it binds, once by id.

"Comments and docs removed" means: every `comments` and `doc` list emptied, and every `Comment` statement dropped. A block left with no statements becomes its result: `{ // note ⏎ x }` is `x`. `strata` shows a hash as its first 8 bytes in hex.

### A definition

```text
definition = interface-strings:list(Item)  Decl     (each body written as Unit)
             body-strings:list(Item)       bodies:list(Expr)
Item       = 2·n:uint  n bytes of UTF-8          a text of n bytes
           | 2·k+1:uint                         the id at index k of the file's id table
```

The interface part comes first: its strings, then the `Decl` with each body replaced by `()` (`Unit`, one byte). The body part follows: the strings the bodies add, with indices continuing after the interface's, then the bodies. `interface_size` in the header is the size of the interface part.

An item of a string table is a text, or a reference by id: one `uint`, whose lowest bit says which. A reference costs one byte for the first 64 ids of the file's table and two up to 8,192. A `str` that is a reference reads as the qualified name the header gives its id. Writing a module file again rewrites the string tables of the definitions it keeps for the new id table; the rest of their bytes is copied. Where a definition's or variant's own name is (a `TypeDecl`'s, a top-level `FnDecl`'s, a `TraitDecl`'s, a `CapabilityDecl`'s, an extern type's), or a name of `from m use x`, it reads short: without its module (without `m.`).

The bodies, in order, are:

- a function's body
- a test's body
- the body of each function of a capability, in order
- the body of each method of a trait, in order (`()` for a method without a default)
- the body of each method of an impl, in order

Types and `use` have none. Everything else is interface: signatures, `requires` / `ensures`, laws, fields, variants, refinements.

## Primitives

| Notation | Encoding |
|---|---|
| `uint` | unsigned LEB128: 7 bits at a time, least significant first, the high bit set on every byte but the last. Shortest form only (no trailing `0x80 … 0x00`), at most 64 bits. `300` is `ac 02`. |
| `int` | a 64-bit signed integer, zigzag-encoded then written as a `uint`: `0, -1, 1, -2, 2, …` become `0, 1, 2, 3, 4, …` (`(n << 1) ^ (n >> 63)`). |
| `float` | the 8 bytes of the IEEE 754 double, little-endian. |
| `bool` | one byte, `0` or `1`. |
| `id` | 4 bytes, least significant first: an unsigned 32-bit integer, never 0 (an `ast.Id`, a `U32`, in memory). |
| `char` | a Unicode scalar value, as a `uint`. |
| `raw-string` | length in bytes (`uint`), then the UTF-8 bytes. Only in a header's string table (and a bundle's manifest); a definition's table has `Item`s. |
| `raw-bytes` | length (`uint`), then the bytes. |
| `str` | a `uint`: the string's index in the current table (from 0). |
| `list(T)` | the number of items (`uint`), then each item. |
| `option(T)` | one byte: `0` for none, `1` followed by the value. |
| `(A, B)` | `A` then `B`. |
| a struct | its fields, in the order listed below. |
| an enum | one tag byte, then the fields of that variant, in order. |

When writing, a string gets the next index the first time it is written, and each table holds its strings in that order. A reader just looks strings up by index.

## The tree

`comments` are the `//` lines above a node (`""` stands for a blank line between groups), `doc` the `///` lines.

### Definitions

```text
Decl        = comments:list(str)  doc:list(str)  module:str  public:bool  kind:DeclKind
DeclKind    = 0 TypeDecl | 1 FnDecl | 2 TestDecl | 3 TraitDecl | 4 ImplDecl | 5 UseDecl | 6 ExternDecl
            | 7 CapabilityDecl | 8 DeriveFnDecl

TypeDecl    = distinct:bool  name:str  params:list(str)  body:TypeBody  refinement:option(Expr)  mock:bool
TypeBody    = 0 Alias(TypeExpr) | 1 Record(list(Field)) | 2 Sum(list(Variant))
Field       = comments:list(str)  doc:list(str)  name:str  ty:TypeExpr  default:option(Expr)
Variant     = comments:list(str)  doc:list(str)  name:str  payload:Payload
Payload     = 0 None | 1 Positional(list(TypeExpr)) | 2 Record(list(Field))
            | 3 Type(TypeExpr)        a union member defined elsewhere (`Circle` in `Circle | Square { .. }`)

FnDecl      = doc:list(str)  comments:list(str)  name:str  type_params:list(str)  bounds:list(Bound)
              params:list(Param)  ret:Result  requires:list(Expr)  ensures:list(Expr)
              flags:byte  body:Expr  comptime:bool  derived:bool
Result      = TypeExpr | 5 names:list(str) TypeExpr     -> (next: Q, added: Int): the names, then the tuple
Param       = name:str  ty:TypeExpr  mutable:bool  comptime:bool
Bound       = param:str  traits:list(TypeExpr)

TestDecl    = name:str  params:list(Param)  body:Expr
TraitDecl   = name:str  params:list(str)  supers:list(TypeExpr)  methods:list(TraitMethod)  laws:list(Law)
              derivable:option(Derivable)  flags:byte  [models:list(ModelDecl)  invariants:list(Expr)  ensures:list(Expr)]
              (flags: 1 a capability trait, 2 the bracketed part follows, 4 a `shared` trait)
ModelDecl   = comments:list(str)  doc:list(str)  name:str  ty:TypeExpr
Derivable   = kind:TypeExpr  requires:list(Expr)
TraitMethod = comments:list(str)  doc:list(str)  decl:FnDecl  default:bool
Law         = comments:list(str)  doc:list(str)  name:str  params:list(Param)  body:Expr
ImplDecl    = type_params:list(str)  bounds:list(Bound)  trait_name:str  trait_args:list(TypeExpr)
              for_type:TypeExpr  methods:list(FnDecl)  flags:byte  [models:list(ModelDef)]  [clauses:list(DeriveClause)]
              (flags: 1 `derive`, 2 the models follow, 4 a derive's clauses follow)
ModelDef    = comments:list(str)  name:str  value:Expr
DeriveClause = comments:list(str)  name:str  names:list(str)    key code, key (shelf, slot)
UseDecl     = path:str  alias:option(str)  items:option(list(UseItem))     use m [as a] (no items), from m use x, y as z
UseItem     = name:str  alias:option(str)
ExternDecl  = lang:Lang  item:ExternItem
Lang        = 0 C | 1 Builtin | 2 Python(module:str) | 3 Rust(crate:str) | 4 Ts(module:str)
CapabilityDecl = name:str  methods:list(FnDecl)
DeriveFnDecl = trait_name:str  function:str  derived:bool  exports:list(str str)  files:list(str str)
ExternItem  = 0 Fn(FnDecl) | 1 Type(name:str)
```

`module` is the dotted module path (`""` for the root module). A top-level function keeps its doc and comments on its `Decl`; `FnDecl.doc` / `comments` are for methods inside an `impl`. An inherent `impl Item { … }` has `trait_name = ""`. `derive Trait for Type { … }` is an `ImplDecl` with `derived` set (its `methods` are the ones written by hand); `comptime fn` and `derived fn` set those flags of `FnDecl`; its `flags` byte is the sum of `1` for `terminates`, `2` for `noalloc` and `4` for a `lemma` (so a file that stored it as a `bool` reads the same), `derivable(t: Record) trait …` the trait's `derivable`. `derive Trait for function` (a lowercase name after `for`) is a `DeriveFnDecl` with `derived` set and no `exports` or `files` (what deriving makes, held only in memory: each exported symbol with its function, each file's extension with the function giving its part); its header `kind` is `8`. `items` is none for `use m` (the module, bound to `alias` or its last name) and the names of `from m use x, y as z` otherwise. An `extern c fn` is an `ExternItem.Fn` whose `FnDecl` has no body (`()`, so it has no body part) and whose name is the C symbol; `extern c type` is `ExternItem.Type`; `extern python "m" fn` is an `ExternItem.Fn` with the `Lang` `Python("m")`. Its header `kind` is `6`. `capability Name { ... }` is a `CapabilityDecl` (its functions written without `self`; their bodies are its body part, in order), `capability trait` sets the trait's `capability`, and a test's `params` are the capabilities it asks for.

### Types

```text
TypeExpr    = 0 Named(name:str, args:list(TypeExpr))      Int, List[T], T
            | 1 Fn(params:list(TypeExpr), ret:TypeExpr)   (A, B) -> C
            | 2 Unit                                      ()
            | 3 Refined(base:TypeExpr, pred:Expr)         Int where it > 0
            | 4 Union(list(TypeExpr))                     A | B
            | 5 Row(list(str TypeExpr))                   { id: Int, total: Cents, .. } (a bound, or a parameter's type)
```

### Expressions

```text
Expr = 0  Int(int)                       1  BigInt(digits:str)       2  Float(float)
     | 3  Tuple(list(Expr))              4  Str(str)                 5  Bool(bool)
     | 6  Unit                           7  Var(str)                 8  Hole(name:str)
     | 9  List(list(Expr))               10 Ctor(name:str, CtorArgs)
     | 11 Call(f:Expr, args:list(Expr))  12 Method(recv:Expr, name:str, args:list(Expr))
     | 13 Field(Expr, name:str)          14 Lambda(list(LamParam), body:Expr)
     | 15 Bin(BinOp, Expr, Expr)         16 Un(UnOp, Expr)           17 Is(Expr, variant:str)
     | 18 If(cond:Expr, then:Expr, else:Expr)
     | 19 Match(scrutinee:Expr, as:option(str), arms:list(Arm))
     | 20 Mut(Expr)                      21 Block(list(Stmt), last:Expr)
     | 22 Mock(TypeExpr)                 23 Return(Expr)             24 Try(Expr, TryKind)
     | 25 Except(Expr, err:option(str), fallback:Expr)          x except e { … }, x !! v
     | 26 Interp(list(StrPart))
     | 27 Range(lo:option(Expr), hi:option(Expr), inclusive:bool)    a..b, a..=b
     | 28 Index(Expr, index:Expr)                                   xs[i], xs[a..b]
     | 29 Comp(CompKind, elem:Expr, list(CompFor))                  [e for x in xs where c]
     | 30 Dict(types:list(TypeExpr), list((Expr, Expr)))            Dict { "a": 1 }
     | 31 Set(types:list(TypeExpr), list(Expr))                     Set { 1, 2 }
     | 32 Or(Expr, fallback:Expr)                                   x or { … }, x ?? v
     | 33 Comptime(Expr)                                            comptime e
     | 34 Scope(Expr)                                               scope { … }
     | 35 Spawn(Expr)                                               spawn e
     | 36 Await(Expr)                                               await h
     | 37 Select(list(SelectArm))                                   select { rx.recv() as x => … }
     | 38 Race(list(SelectArm))                                     race { e as x => … }
     | 39 SupervisedSpawn(Expr)                                     supervised spawn e
     | 40 Old(Expr)                                                 old e (in an `ensures`: e where the function started)
     | 41 Assumed(Expr)                                             assumed requires/ensures/invariant e (verify only)
     | 42 Spread(Expr)                                              ...e (a call's argument: the record's fields given to the parameters of their names)

CtorArgs = 0 None | 1 Positional(list(Expr)) | 2 Record(list((str, Expr)))
         | 3 Update(base:Expr, list((str, Expr)))
LamParam = name:str  ty:option(TypeExpr)
CompFor  = pat:Pattern  iter:Expr  cond:option(Expr)
CompKind = 0 List | 1 Set | 2 Dict        (a Dict comprehension's element is a Tuple of the key and value)
SelectArm = comments:list(str)  op:Expr  name:option(str)  body:Expr      (in a `select`, `op` is `rx.recv()`, `tx.send(v)` or `await h`; in a `race`, any expression, run as a task)
Arm      = comments:list(str)  pat:Pattern  body:Expr
StrPart  = 0 Lit(str) | 1 Expr(Expr, spec:option(FmtSpec))
FmtSpec  = fill:char  align:option(char)  plus:bool  zero:bool  width:uint  precision:option(uint)

BinOp    = 0 +  1 -  2 *  3 /  4 %  5 ==  6 !=  7 <  8 <=  9 >  10 >=  11 &&  12 ||  13 ==>
         | 14 &  15 |  16 ^  17 <<  18 >>
UnOp     = 0 -  1 !
TryKind  = 0 `!` (Result)  1 `?` (Option)
```

`BigInt` is an integer literal too large for 64 bits, as its decimal digits.

### Statements and patterns

```text
Stmt    = 0 Let(Pattern, option(TypeExpr), Expr)   1 Var(name:str, option(TypeExpr), Expr)
        | 2 Assign(place:Expr, Expr)               3 While(cond:Expr, body:Expr)
        | 4 For(Pattern, Expr, body:Expr)          5 Loop(body:Expr)
        | 6 Break   7 Continue   8 Expr(Expr)   9 Assert(Expr)   10 Comment(list(str))
        | 11 ComptimeFor(Pattern, Expr, body:Expr)          comptime for v in Self.members { … }
        | 12 Invariant(Expr)                                 a loop's `invariant e`, first in its body
        | 13 Decreases(Expr)                                 a loop's `decreases e`, first in its body (with
                                                             its invariants); a function's, its body's first
        | 14 AssertBy(cond:Expr, proof:Expr)                 assert e by lemma(args), assert e by { steps } (verify only)
        | 15 Calc(list(Expr))                                calc { a == b; _ <= c } (verify only; `_` is Var("_"))

Pattern = 0 Wild   1 Bind(str)   2 Int(int)   3 Str(str)   4 Bool(bool)
        | 5 Ctor(name:str, CtorPat)
        | 6 List(list(Pattern), rest:option(str))     [a, ..rest]
        | 7 Tuple(list(Pattern))   8 Cond(Expr)   9 Or(list(Pattern))
        | 10 For(arm:Pattern, each:Pattern, list:Expr)     v => e for v in Self.members
CtorPat = 0 None | 1 Positional(list(Pattern)) | 2 Record(list((str, Pattern)), dots:bool)
        | 3 Type(list(Pattern))                          a type's arguments: List[Int], Option[x]
```

## An example

`fn one() -> Int { 1 }`, a script, is stored as a 155-byte file:

```text
53 54 52 41 54 41   "STRATA"
01 00               version 1, a module file
header:
   02 00 03 6f 6e 65    2 strings: "", "one"
   00                   module "" (the root)
   00                   no variants
   00                   no references to other modules
   01                   1 entry:
      99 e9 81 f2          id (key 0 of the file's id table: module "", this id)
      01 01 00             name "one", a function, private
      f1 c8 b0 … (32)      hash (of the named bytes)
      e8 95 15 … (32)      interface_hash
      f1 c8 b0 … (32)      code_hash (the same as hash: no comments)
      00                   no dependencies
      1f 1b                31 bytes, 27 of them interface
the definition:
   03                              3 strings:
      00                              "" (a text of 0 bytes)
      01                              the id at index 0: `one` (its own name)
      06 49 6e 74                     "Int" (a text of 3 bytes)
   00 00 00 00 01                  no comments or doc, module "", private; a function:
   00 00 01 00 00 00               no doc, no comments, name: string 1 (the id of `one`), no type parameters, bounds, parameters
   00 02 00                        returns Named("Int", [])
   00 00 00                        no requires, no ensures, no flags (not `terminates`, not `noalloc`)
   06                              its body, left out: ()
   00 00                           not `comptime`, not derived
   00                              the bodies add no strings
   01 00 02                        1 body: Int(1)  (1 zigzag-encodes to 2)
```

(The id is random; the hashes shown are examples.)

## References by id

Every definition and every variant of a sum has an id: 32 random bits, never 0 (`arc4random_buf`, `binary.new_id`), an `ast.Id` (a `U32`; the width is that type, `binary.id_bytes`, `byte_id` and `new_id`, and c/cache.c's `binary_new_id`). An id is unique within its module: a reference names a module and an id (`ast.Key`). It is given by the encoder to a definition that has none (`binary.assign_ids`) and kept in `Decl.id` and `Variant.id` in memory. Writing a module file again keeps each definition's id: one without an id (just parsed from text) takes the id of the definition of the same module, name and kind in the previous version or in the file, else a new one. An id another definition of the module already has is drawn again. So `edit` keeps a definition's id when it replaces it, and `rename` keeps it as it changes the name; `move` keeps it too unless the new module has it already, but a moved definition is another key, and what refers to it is written again by `move`. A definition deleted and written again under another name with `edit` is a new definition with a new id; only `rename` and `move` keep one across names.

What is written by id. In every place that can name a definition (a variable, a constructor, a type's name, a pattern's constructor, an `is` variant, an impl's trait, a derive's trait and function, a definition's own name, a variant's name, a name of `from m use x`), a qualified name that is the name of a known id is written as a reference to that id. The ids known are the program's (every module of the package) and the ones std and other packages give the names the file already refers to. Three cases stay text:

- a name no id has: builtins, core's global names, names that resolve to nothing, and a name two definitions give (a builtin's overloads, a variant named as a type of its module);
- a bare name (the root module's) that the definition also binds as a parameter, local, field or type parameter, anywhere in it: the encoder can't tell which one a use means, so it keeps the text (the named pass lists the names a definition binds);
- trait methods, impl methods, fields and locals, which are named by text.

Reading. A file is read with its own header: each id as the header names it. A package is read with every module's header first (`decoded_files`), and each reference is read as the name the module that has its key gives it, which differs from the referring file's when the two were written apart: a branch renamed the definition, another added a use, and git merged the two files without the driver. One case is told apart: when the referring file's name for the key is still the name of a definition of the package, the owner gave that definition another id (a merge does, see below), and the reference keeps its name (`binary.owner_names`). The referring file's copy of the name is brought up to date the next time it is written (`strata upgrade` rewrites every file).

What it gives:

- A rename changes the name in one header. Every other module's definitions keep their bytes; each module that refers to it has its own copy of the name in its header, which is updated.
- `strata diff` pairs a definition, and a variant, by its key: one with the same module and id and a new name is renamed, even if its body changed too. A move is another key: it is reported from the log.
- `strata merge-file` reads each id by one name on all three sides (the name one side changed it to, ours first), so a rename on one side is followed by the other side's new uses with nothing guessed. Two new definitions with one name, one on each side, have different ids: a name clash, reported as a conflict. Two definitions of the module with one id, one from each side (two branches drew the same 32 bits), keep it for the first and draw another for the second, and merge-file says so.
- `strata sync` after a move written apart from new uses moves them too (the old name is read for the old key, and the move is done again on the merged code).

Limits: an edit that adds the first reference to a std name in a package stores it as text until `strata upgrade` (the ids of std are known only for names a module already refers to); `strata resolve FILE`, which stores a conflict resolved by hand from text, gives that module's definitions new ids.

## Changing the format

Code is stored in this format, so a change to the tree (a new node, a new field) changes the files of every package and script in the repository. Until the format is settled, `VERSION` stays `1`, and the files are converted in the same change:

1. With the strata from before the change, write each file's text: `strata view main --file F --proofs > F.st` (`--proofs`: with the proof material the default view hides), or `view --split DIR` for a package.
2. Make the change and build.
3. Read the text back in: `strata --file F edit < F.st` for a script, or `strata --file PKG/ edit MODULE < M.st` for each module of a package. Do the standard library first; its modules are `std.io`, …, and the compiled prelude, `runtime/prelude.strata`, is module `strata.compiled`.
4. Run `strata upgrade`, which rewrites every package and script in the repository in the current form, updating the headers' dependency lists, and prints each file it rewrites.

Adding a node at the end of a list of tags keeps existing files readable, so it needs no conversion. Adding a field to a node, or changing a node, does. Once the format is settled, the version will count changes, and the reader may accept older versions.
