# The binary format

A whole Strata program, and its package's `strata.toml`, can be stored as one binary file: the syntax tree itself, not its text. `strata pack app.stb` writes one; `--file app.stb`, `strata run app.stb`, `strata ui --file app.stb` and every view read it as they read source; `strata unpack app.stb DIR` writes it back out as source, one canonical file per module.

The format is our own, written by hand in `src/binary.rs` with no serialization library, so that Strata can read and write it itself once it is bootstrapped. This page is its specification; the two are kept in step.

Properties:

- **Exact.** Decoding gives back the very tree that was encoded: comments, docs, blank lines between groups, big integers (as their digits) and floats (as their bits).
- **Canonical.** A tree has one encoding: strings are numbered in the order they are first written, integers use the shortest form, and the decoder refuses anything else. Packing what you unpacked gives the same bytes.
- **Compact.** Every name, comment and string literal is stored once, in a table, and referred to by number. `examples/tasks` is 20 KB of source and 14 KB packed.
- **Safe to read.** A truncated or corrupt file is an error naming the byte where it went wrong, never a crash: lengths can't exceed what is left of the file, and nesting deeper than 10,000 levels is refused.

There is no backwards compatibility: a file of another format version is rejected. Re-pack it from source.

## Layout

```text
file      = "STRATA"  version:uint  strings:list(raw-string)  manifest:option(str)  decls:list(Decl)
```

- the six ASCII bytes `STRATA` (`53 54 52 41 54 41`);
- the format `version`, currently `1`;
- the string table: its length, then each string as its length in bytes and its UTF-8 bytes;
- the package's `strata.toml`, as written (`None` for a script);
- the program's definitions, in order.

Nothing may follow the last definition.

## Primitives

| Notation | Encoding |
|---|---|
| `uint` | unsigned LEB128: 7 bits at a time, least significant first, the high bit set on every byte but the last. Shortest form only (no trailing `0x80 … 0x00`), at most 64 bits. `300` is `ac 02`. |
| `int` | a 64-bit signed integer, zigzag-encoded then written as a `uint`: `0, -1, 1, -2, 2, …` become `0, 1, 2, 3, 4, …` (`(n << 1) ^ (n >> 63)`). |
| `float` | the 8 bytes of the IEEE 754 double, little-endian. |
| `bool` | one byte, `0` or `1`. |
| `char` | a Unicode scalar value, as a `uint`. |
| `raw-string` | length in bytes (`uint`), then the UTF-8 bytes. Only in the string table. |
| `str` | a `uint`: the string's index in the table (from 0). |
| `list(T)` | the number of items (`uint`), then each item. |
| `option(T)` | one byte: `0` for none, `1` followed by the value. |
| `(A, B)` | `A` then `B`. |
| a struct | its fields, in the order listed below. |
| an enum | one tag byte, then the fields of that variant, in order. |

When writing, a string gets the next index the first time it is written; the table holds the strings in that order. Whatever the order, a reader just looks strings up by index.

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

CtorArgs = 0 None | 1 Positional(list(Expr)) | 2 Record(list((str, Expr)))
         | 3 Update(base:Expr, list((str, Expr)))
LamParam = name:str  ty:option(TypeExpr)
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

`fn one() -> Int { 1 }`, a script, encodes to 38 bytes:

```text
53 54 52 41 54 41   "STRATA"
01                  version 1
03                  3 strings:
   00                 0: ""
   03 6f 6e 65        1: "one"
   03 49 6e 74        2: "Int"
00                  no manifest
01                  1 definition:
   00 00 00 00          no comments, no doc, module "" (string 0), not public
   01                   a function:
   00 00 01             no doc, no comments, name "one" (string 1)
   00 00 00             no type parameters, bounds or parameters
   00 02 00             returns Named("Int", [])
   00 00 00             no requires, no ensures, not `terminates`
   00 02                body: Int(1)  (1 zigzag-encodes to 2)
```

(The string table is in order of first use: the module `""` comes before the name, as `Decl.module` is written before `FnDecl.name`.)
