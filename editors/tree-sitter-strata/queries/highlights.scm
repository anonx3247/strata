; Strata highlights. Capture names follow Helix's scopes (an editor that
; lacks one falls back to the part before its last dot). The last pattern
; that matches a node wins, as in Helix, Neovim and `tree-sitter highlight`:
; plain names first, the specific ones after.

(identifier) @variable

(type_identifier) @type

; ---------- names ----------

((identifier) @variable.builtin
  (#any-of? @variable.builtin "it" "result"))

(self) @variable.builtin
((identifier) @variable.builtin
  (#eq? @variable.builtin "self"))

((type_identifier) @type.builtin
  (#any-of? @type.builtin
    "Int" "Float" "Bool" "String" "Unit" "List" "Set" "Dict" "Option" "Result"
    "I8" "I16" "I32" "I64" "U8" "U16" "U32" "U64" "Self"))

(field_identifier) @variable.other.member

(parameter name: (identifier) @variable.parameter)
(lambda_parameter name: (identifier) @variable.parameter)
(lambda_expression parameters: (identifier) @variable.parameter)
(type_parameter name: (type_identifier) @type.parameter)

(module_path (identifier) @namespace)
(scoped_type_identifier path: (identifier) @namespace)

; ---------- definitions and calls ----------

(function_declaration name: (identifier) @function)
(extern_declaration name: (identifier) @function)
(builtin_declaration name: (identifier) @function)
(law_declaration name: (identifier) @function)

(call_expression function: (identifier) @function)
(call_expression
  function: (field_expression field: (field_identifier) @function.method))

; constructors: `Some(x)`, `Point { … }`, `Dot`, and in patterns
(call_expression function: (type_identifier) @constructor)
(record_expression name: (type_identifier) @constructor)
(record_expression name: (scoped_type_identifier name: (type_identifier) @constructor))
(constructor_pattern name: (type_identifier) @constructor)
(constructor_pattern name: (scoped_type_identifier name: (type_identifier) @constructor))
(variant name: (type_identifier) @constructor)

; ---------- literals ----------

(integer) @constant.numeric.integer
(negative_integer) @constant.numeric.integer
(float) @constant.numeric.float
(boolean) @constant.builtin.boolean
(unit) @constant.builtin

(string) @string
(f_string) @string
(escape_sequence) @constant.character.escape
(format_spec) @string.special
(interpolation
  "{" @punctuation.special
  "}" @punctuation.special)

(hole) @special

(comment) @comment.line
(doc_comment) @comment.line.documentation

; ---------- keywords ----------

[
  "use"
  "from"
  "as"
] @keyword.control.import

[
  "type"
  "distinct"
  "mock"
  "trait"
  "impl"
  "derive"
  "derivable"
  "capability"
  "extern"
  "builtin"
  "static"
  "thread"
  "law"
  "model"
  "test"
] @keyword.storage.type

"fn" @keyword.function

[
  (visibility_modifier)
  "mut"
  "comptime"
  "derived"
] @keyword.storage.modifier

[
  "let"
  "where"
  "is"
  "in"
  "assert"
  "invariant"
] @keyword

[
  "ghost"
  "requires"
  "ensures"
  "terminates"
  "decreases"
  "old"
] @keyword.directive

[
  "if"
  "else"
  "match"
] @keyword.control.conditional

[
  "while"
  "for"
  "loop"
  (break_statement)
  (continue_statement)
] @keyword.control.repeat

[
  "return"
] @keyword.control.return

[
  "except"
  "or"
] @keyword.control.exception

[
  "spawn"
  "await"
  "scope"
  "select"
  "race"
  (supervised_spawn)
] @keyword.control

(select_arm supervised: (identifier) @keyword.control)

[
  "Dict"
  "Set"
] @type.builtin

(wildcard_pattern) @variable.builtin

; ---------- operators and punctuation ----------

[
  "+" "-" "*" "/" "%"
  "==" "!=" "<" "<=" ">" ">="
  "&&" "||" "!" "!!" "??" "?" "==>"
  "&" "|" "^" "<<" ">>"
  "=" ":=" "+=" "-=" "*=" "/=" "%="
  ".." "..="
  "->" "=>"
] @operator

["(" ")" "[" "]" "{" "}"] @punctuation.bracket

["," "." ":" ";"] @punctuation.delimiter
