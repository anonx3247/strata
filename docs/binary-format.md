# The binary format

Strata code is stored as its syntax tree, not its text, in a binary format of our own: a `.strata` file is a **module file** (`main.strata`, `shop/orders.strata`), or a whole program in one **bundle** file. Text is a view of it.

- **Text goes in through an edit.** `strata edit` (from stdin or an editor) and `strata add FILE` parse text and store the result. Nothing else reads text: a `.strata` file that holds text is refused.
- **Text comes out through `view`.** `strata view MODULE` prints a module as stored (imports first), `view --split DIR` writes one text file per module (`shop/orders.st`), and `view SELECTOR --level 0|1|2` shows definitions at a level of detail.
- `strata pack app.strata` writes a bundle; `--file`, `run`, `ui` and every view read it like a package. `strata pack --prelude FILE` writes the prelude's definitions (its types and code, under the names programs use) as a module file, for Strata code that runs trees (`compiler/eval`).
- `strata header core.strata` prints a module file's table of contents; `strata header core.strata --tree` prints each definition (a bundle's too) as a `compiler/ast` value, one per line, and `--text` as the printer writes it, as stored (references qualified: `view` shows them shortened).
- In git, `strata setup-git` makes `git diff` and `git log -p` show `.strata` files as text, and merges them definition by definition (a conflict is left as text; `strata resolve FILE` stores it again once fixed).

The format is written by hand, with no serialization library, so that Strata can read and write it itself: `compiler/binary` does (in Strata, on the tree `compiler/ast`, hashes included, computed with macOS's SHA-256, CommonCrypto's `CC_SHA256`, through `compiler/c/cache.c`); every module file of the repository round-trips through it (`cli-every_example_round_trips_through_render`). This page is its specification; the two are kept in step.

Design:

- **A header, like a C header file, but inside the module file.** It comes first and lists every definition: its name, kind, visibility, three hashes, the definitions it depends on and the size of its bytes. Listing a module, building the dependency graph or finding what changed reads only the header. That takes 14 µs for all of `examples/tasks`; decoding everything takes about 170 µs.
- **Interface first, bodies after.** Each definition's bytes start with its interface: types, signatures, contracts, laws, with each body written as `()`. Its bodies follow. Checking code against a module only needs the interfaces, and a reader can stop there.
- **Self-contained definitions.** Each definition carries its own strings, so its bytes mean the same in any file. Its hash is the hash of those bytes. The same bytes could also be stored as one object under that hash (the hash-store idea in IDEAS.md).
- **References by what they name.** Every reference to a definition is stored by its qualified name (`orders.Line`), however it was written (`Line` under `from orders use Line`, or `orders.Line` under `use orders`); a call to another module's function is stored as a call of that name (`orders.ship(x)` is `Call(Var("orders.ship"), [x])`, not a method call on `orders`). A definition's own name, its variants and methods, locals, fields and method calls (found from their receiver) are stored as written, and so is a name that resolves to nothing. When code is shown, each reference is written as short as its module's `use`s allow, so text is normalised: `orders.Line` written where `Line` is imported shows as `Line`. A module file shown alone (`git diff`) decides from its own `use` lines. In `compiler/resolve`: `stored` and `written`.
- **Exact.** Decoding gives back the very tree that was encoded: comments, docs, blank lines between groups, big integers (as their digits) and floats (as their bits).
- **Canonical.** A tree has one encoding. Strings are numbered in the order they are first written, integers use the shortest form, and the decoder refuses anything else. A module file's bytes are what strata writes for it (a header's dependency lists can go stale when another module changes outside strata, or when an edit changes what a definition it leaves alone would now depend on: an edit writes only the files it changes, and in them keeps each untouched definition's bytes and entry as stored; `strata upgrade` rewrites them all).
- **Safe to read.** A truncated or corrupt file is an error, never a crash. Lengths can't exceed what is left of the file, nesting deeper than 10,000 levels is refused, and a definition that isn't in the file's module, or isn't of the kind and visibility its header entry says, is rejected. A definition's hashes are computed when it is stored and trusted when it is read (reading doesn't hash every definition again); `binary.read_checked` also checks each definition's bytes against its hash, which catches any change inside a definition (`compiler-bad_input`).

A file of another format version is rejected. See [Changing the format](#changing-the-format).

## Files

```text
module-file = "STRATA" version:uint 0  header  definitions
bundle      = "STRATA" version:uint 1  manifest:option(raw-string)  modules:list(raw-bytes)
```

- the six ASCII bytes `STRATA` (`53 54 52 41 54 41`), the format `version` (`1`: the format is not settled, so changes to it convert the files rather than bump it), then `0` for a module file or `1` for a bundle;
- a bundle holds the package's `strata.toml`, as written (none for a script), then each module file, as its length and its bytes, in the order the modules first appear in the program.

### The header

```text
header = strings:list(raw-string)  module:str  entries:list(Entry)
Entry  = name:str  kind:byte  public:bool  hash:32 bytes  interface_hash:32 bytes  code_hash:32 bytes
         deps:list(str)  size:uint  interface_size:uint
```

- The header's strings are its own table (names, the module path, dependencies).
- `module` is the dotted module path (`""` for the root module).
- `name` is what `strata` calls the definition: `total`, `Order`, `test "totals"`, `impl Show for Order`, `use orders`.
- `kind` is the `DeclKind` tag below.
- `deps` are the qualified names of the definitions it refers to, sorted.
- The definitions follow the header back to back, in the order of the entries. A definition's offset is the end of the header plus the sizes of those before it, and the file ends exactly after the last one.

The three hashes are SHA-256:

| Hash | Of | Changes when |
|---|---|---|
| `hash` | the definition's bytes | anything changes, a comment included: its identity |
| `interface_hash` | the interface part of the definition with every comment and doc removed | what other code relies on changes: types, signatures, contracts, laws, visibility, its module |
| `code_hash` | the bytes of the definition with every comment and doc removed | what it does changes (verification results are keyed by it) |

"Comments and docs removed" means: every `comments` and `doc` list emptied, and every `Comment` statement dropped. A block left with no statements becomes its result: `{ // note ⏎ x }` is `x`. `strata` shows a hash as its first 8 bytes in hex.

### A definition

```text
definition = interface-strings:list(raw-string)  Decl     (each body written as Unit)
             body-strings:list(raw-string)       bodies:list(Expr)
```

The interface part comes first: its strings, then the `Decl` with each body replaced by `()` (`Unit`, one byte). The body part follows: the strings the bodies add (their indices continue after the interface's), then the bodies. `interface_size` in the header is the size of the interface part.

The bodies, in order, are:
- a function's body;
- a test's body;
- the body of each function of a capability, in order;
- the body of each method of a trait, in order (a method without a default has `()`);
- the body of each method of an impl, in order.

Types and `use` have none. Everything else is interface: signatures, `requires` / `ensures`, laws, fields, variants, refinements.

## Primitives

| Notation | Encoding |
|---|---|
| `uint` | unsigned LEB128: 7 bits at a time, least significant first, the high bit set on every byte but the last. Shortest form only (no trailing `0x80 … 0x00`), at most 64 bits. `300` is `ac 02`. |
| `int` | a 64-bit signed integer, zigzag-encoded then written as a `uint`: `0, -1, 1, -2, 2, …` become `0, 1, 2, 3, 4, …` (`(n << 1) ^ (n >> 63)`). |
| `float` | the 8 bytes of the IEEE 754 double, little-endian. |
| `bool` | one byte, `0` or `1`. |
| `char` | a Unicode scalar value, as a `uint`. |
| `raw-string` | length in bytes (`uint`), then the UTF-8 bytes. Only in string tables (and a bundle's manifest). |
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
            | 7 CapabilityDecl

TypeDecl    = distinct:bool  name:str  params:list(str)  body:TypeBody  refinement:option(Expr)  mock:bool
TypeBody    = 0 Alias(TypeExpr) | 1 Record(list(Field)) | 2 Sum(list(Variant))
Field       = comments:list(str)  doc:list(str)  name:str  ty:TypeExpr  default:option(Expr)
Variant     = comments:list(str)  doc:list(str)  name:str  payload:Payload
Payload     = 0 None | 1 Positional(list(TypeExpr)) | 2 Record(list(Field))
            | 3 Type(TypeExpr)        a union member defined elsewhere (`Circle` in `Circle | Square { .. }`)

FnDecl      = doc:list(str)  comments:list(str)  name:str  type_params:list(str)  bounds:list(Bound)
              params:list(Param)  ret:TypeExpr  requires:list(Expr)  ensures:list(Expr)
              terminates:bool  body:Expr  comptime:bool  derived:bool
Param       = name:str  ty:TypeExpr  mutable:bool  comptime:bool
Bound       = param:str  traits:list(TypeExpr)

TestDecl    = name:str  params:list(Param)  body:Expr
TraitDecl   = name:str  params:list(str)  supers:list(TypeExpr)  methods:list(TraitMethod)  laws:list(Law)
              derivable:option(Derivable)  capability:bool
Derivable   = kind:TypeExpr  requires:list(Expr)
TraitMethod = comments:list(str)  doc:list(str)  decl:FnDecl  default:bool
Law         = comments:list(str)  doc:list(str)  name:str  params:list(Param)  body:Expr
ImplDecl    = type_params:list(str)  bounds:list(Bound)  trait_name:str  trait_args:list(TypeExpr)
              for_type:TypeExpr  methods:list(FnDecl)  derived:bool
UseDecl     = path:str  alias:option(str)  items:option(list(UseItem))     use m [as a] (no items), from m use x, y as z
UseItem     = name:str  alias:option(str)
ExternDecl  = lang:Lang  item:ExternItem
Lang        = 0 C
CapabilityDecl = name:str  methods:list(FnDecl)
ExternItem  = 0 Fn(FnDecl) | 1 Type(name:str)
```

`module` is the dotted module path (`""` for the root module). A top-level function keeps its doc and comments on its `Decl`; `FnDecl.doc` / `comments` are for methods inside an `impl`. An inherent `impl Item { … }` has `trait_name = ""`. `derive Trait for Type { … }` is an `ImplDecl` with `derived` set (its `methods` are the ones written by hand); `comptime fn` and `derived fn` set those flags of `FnDecl`, `derivable(t: Record) trait …` the trait's `derivable`. `items` is none for `use m` (the module, bound to `alias` or its last name) and the names of `from m use x, y as z` otherwise. An `extern c fn` is an `ExternItem.Fn` whose `FnDecl` has no body (`()`, so it has no body part) and whose name is the C symbol; `extern c type` is `ExternItem.Type`. Its header `kind` is `6`. `capability Name { ... }` is a `CapabilityDecl` (its functions written without `self`; their bodies are its body part, in order), `capability trait` sets the trait's `capability`, and a test's `params` are the capabilities it asks for.

### Types

```text
TypeExpr    = 0 Named(name:str, args:list(TypeExpr))      Int, List[T], T
            | 1 Fn(params:list(TypeExpr), ret:TypeExpr)   (A, B) -> C
            | 2 Unit                                      ()
            | 3 Refined(base:TypeExpr, pred:Expr)         Int where it > 0
            | 4 Union(list(TypeExpr))                     A | B
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

Pattern = 0 Wild   1 Bind(str)   2 Int(int)   3 Str(str)   4 Bool(bool)
        | 5 Ctor(name:str, CtorPat)
        | 6 List(list(Pattern), rest:option(str))     [a, ..rest]
        | 7 Tuple(list(Pattern))   8 Cond(Expr)   9 Or(list(Pattern))
        | 10 For(arm:Pattern, each:Pattern, list:Expr)     v => e for v in Self.members
CtorPat = 0 None | 1 Positional(list(Pattern)) | 2 Record(list((str, Pattern)), dots:bool)
        | 3 Type(list(Pattern))                          a type's arguments: List[Int], Option[x]
```

## An example

`fn one() -> Int { 1 }`, a script, is stored as a 152-byte file:

```text
53 54 52 41 54 41   "STRATA"
01 00               version 1, a module file
header:
   02 00 03 6f 6e 65    2 strings: "", "one"
   00                   module "" (the root)
   01                   1 entry:
      01 01 00             name "one", a function, private
      10 f4 ce … (32)      hash
      a1 4f dd … (32)      interface_hash
      10 f4 ce … (32)      code_hash (the same as hash: no comments)
      00                   no dependencies
      22 1e                34 bytes, 30 of them interface
the definition:
   03 00 03 6f 6e 65 03 49 6e 74   3 strings: "", "one", "Int"
   00 00 00 00 01                  no comments or doc, module "", private; a function:
   00 00 01 00 00 00               no doc, no comments, name "one", no type parameters, bounds, parameters
   00 02 00                        returns Named("Int", [])
   00 00 00                        no requires, no ensures, not `terminates`
   06                              its body, left out: ()
   00 00                           not `comptime`, not derived
   00                              the bodies add no strings
   01 00 02                        1 body: Int(1)  (1 zigzag-encodes to 2)
```

## Changing the format

The code is stored in this format, so a change to the tree (a new node, a new field) changes the files of every package and script in the repository. Until the format is settled, `VERSION` stays `1`: there is nothing to stay compatible with. Convert the files in the same change:

1. with the strata of before the change, write each file's text: `strata view main --file F > F.st` (`view --split DIR` for a package; a strata from before `view` printed modules: `render`);
2. make the change and build;
3. read the text back in: `strata --file F edit < F.st` for a script, `strata --file PKG/ edit MODULE < M.st` for each module of a package (the standard library first: its modules are `std.io`, …; the compiled prelude, `runtime/prelude.strata`, is module `strata.compiled`);
4. `strata upgrade` rewrites every package and script of the repository in the current form, bringing the headers' dependency lists up to date (it prints each file it rewrites).

Adding a node at the end of a list of tags keeps old files readable, so it needs no conversion; adding a field to a node, or changing a node, does. Once the format is settled, the version will count changes, and the reader may accept older versions.
