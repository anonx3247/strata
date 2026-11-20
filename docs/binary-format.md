# The binary format

Strata code can be stored as its syntax tree rather than its text, in a binary format of our own: one **module file** per module (`main.stb`, `shop/orders.stb`), or a whole program in one **bundle** file.

- `strata pack DIR` writes a folder: the package's `strata.toml` and one module file per module. `strata pack app.stb` writes a bundle.
- `--file DIR`, `--file app.stb`, `strata run …`, `strata ui --file …` and every view read either as they read source.
- `strata unpack IN DIR` writes source back out, one canonical file per module.
- `strata header core.stb` prints a module file's table of contents.

The format is written by hand in `src/binary.rs`, with no serialization library, so that Strata can read and write it itself once it is bootstrapped. This page is its specification; the two are kept in step.

Design:

- **A header, like a C header file, but inside the module file.** It comes first and lists every definition: its name, kind, visibility, three hashes, the definitions it depends on and the size of its bytes. Listing a module, building the dependency graph or finding what changed reads only the header. That takes 14 µs for all of `examples/tasks`; decoding everything takes about 170 µs.
- **Interface first, bodies after.** Each definition's bytes start with its interface: types, signatures, contracts, laws, with each body written as `()`. Its bodies follow. Checking code against a module only needs the interfaces, and a reader can stop there.
- **Self-contained definitions.** Each definition carries its own strings, so its bytes mean the same in any file. Its hash is the hash of those bytes. The same bytes could also be stored as one object under that hash (the hash-store idea in IDEAS.md).
- **Exact.** Decoding gives back the very tree that was encoded: comments, docs, blank lines between groups, big integers (as their digits) and floats (as their bits).
- **Canonical.** A tree has one encoding. Strings are numbered in the order they are first written, integers use the shortest form, and the decoder refuses anything else. Packing what you unpacked gives the same bytes.
- **Safe to read.** A truncated or corrupt file is an error, never a crash. Lengths can't exceed what is left of the file, nesting deeper than 10,000 levels is refused, and a definition whose bytes don't match the hash in the header is rejected.

There is no backwards compatibility: a file of another format version is rejected. Re-pack it from source.

## Files

```text
module-file = "STRATA" version:uint 0  header  definitions
bundle      = "STRATA" version:uint 1  manifest:option(raw-string)  modules:list(raw-bytes)
```

- the six ASCII bytes `STRATA` (`53 54 52 41 54 41`), the format `version` (currently `3`), then `0` for a module file or `1` for a bundle;
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
- `deps` are the qualified names of the definitions it refers to.
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
DeclKind    = 0 TypeDecl | 1 FnDecl | 2 TestDecl | 3 TraitDecl | 4 ImplDecl | 5 UseDecl

TypeDecl    = distinct:bool  name:str  params:list(str)  body:TypeBody  refinement:option(Expr)
TypeBody    = 0 Alias(TypeExpr) | 1 Record(list(Field)) | 2 Sum(list(Variant))
Field       = comments:list(str)  doc:list(str)  name:str  ty:TypeExpr
Variant     = comments:list(str)  name:str  payload:Payload
Payload     = 0 None | 1 Positional(list(TypeExpr)) | 2 Record(list(Field))

FnDecl      = doc:list(str)  comments:list(str)  name:str  type_params:list(str)  bounds:list(Bound)
              params:list(Param)  ret:TypeExpr  requires:list(Expr)  ensures:list(Expr)
              terminates:bool  body:Expr
Param       = name:str  ty:TypeExpr  mutable:bool
Bound       = param:str  traits:list(TypeExpr)

TestDecl    = name:str  body:Expr
TraitDecl   = name:str  params:list(str)  supers:list(TypeExpr)  methods:list(TraitMethod)  laws:list(Law)
TraitMethod = comments:list(str)  doc:list(str)  decl:FnDecl  default:bool
Law         = comments:list(str)  doc:list(str)  name:str  params:list(Param)  body:Expr
ImplDecl    = type_params:list(str)  bounds:list(Bound)  trait_name:str  trait_args:list(TypeExpr)
              for_type:TypeExpr  methods:list(FnDecl)
UseDecl     = path:str  items:option(list(str))
```

`module` is the dotted module path (`""` for the root module). A top-level function keeps its doc and comments on its `Decl`; `FnDecl.doc` / `comments` are for methods inside an `impl`. An inherent `impl Item { … }` has `trait_name = ""`. `items` is none for `use m.*`.

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
     | 25 Except(Expr, err:option(str), fallback:Expr)
     | 26 Interp(list(StrPart))
     | 27 Range(lo:option(Expr), hi:option(Expr), inclusive:bool)    a..b, a..=b
     | 28 Index(Expr, index:Expr)                                   xs[i], xs[a..b]
     | 29 Comp(elem:Expr, list(CompFor))                            [e for x in xs where c]

CtorArgs = 0 None | 1 Positional(list(Expr)) | 2 Record(list((str, Expr)))
         | 3 Update(base:Expr, list((str, Expr)))
LamParam = name:str  ty:option(TypeExpr)
CompFor  = pat:Pattern  iter:Expr  cond:option(Expr)
Arm      = comments:list(str)  pat:Pattern  body:Expr
StrPart  = 0 Lit(str) | 1 Expr(Expr, spec:option(FmtSpec))
FmtSpec  = fill:char  align:option(char)  plus:bool  zero:bool  width:uint  precision:option(uint)

BinOp    = 0 +  1 -  2 *  3 /  4 %  5 ==  6 !=  7 <  8 <=  9 >  10 >=  11 &&  12 ||  13 ==>
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

Pattern = 0 Wild   1 Bind(str)   2 Int(int)   3 Str(str)   4 Bool(bool)
        | 5 Ctor(name:str, CtorPat)
        | 6 List(list(Pattern), rest:option(str))     [a, ..rest]
        | 7 Tuple(list(Pattern))   8 Cond(Expr)   9 Or(list(Pattern))
CtorPat = 0 None | 1 Positional(list(Pattern)) | 2 Record(list((str, Pattern)), dots:bool)
```

## An example

`fn one() -> Int { 1 }`, a script, packs to a 150-byte `main.stb`:

```text
53 54 52 41 54 41   "STRATA"
03 00               version 3, a module file
header:
   02 00 03 6f 6e 65    2 strings: "", "one"
   00                   module "" (the root)
   01                   1 entry:
      01 01 00             name "one", a function, private
      7a 3f c5 … (32)      hash
      ce 72 21 … (32)      interface_hash
      7a 3f c5 … (32)      code_hash (the same as hash: no comments)
      00                   no dependencies
      20 1c                32 bytes, 28 of them interface
the definition:
   03 00 03 6f 6e 65 03 49 6e 74   3 strings: "", "one", "Int"
   00 00 00 00 01                  no comments or doc, module "", private; a function:
   00 00 01 00 00 00               no doc, no comments, name "one", no type parameters, bounds, parameters
   00 02 00                        returns Named("Int", [])
   00 00 00                        no requires, no ensures, not `terminates`
   06                              its body, left out: ()
   00                              the bodies add no strings
   01 00 02                        1 body: Int(1)  (1 zigzag-encodes to 2)
```
