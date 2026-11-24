; Scopes, definitions and references, for editors that highlight a name
; by where it is defined.

[
  (function_declaration)
  (lambda_expression)
  (block)
  (match_arm)
  (for_statement)
  (comptime_for_statement)
  (list_comprehension)
  (law_declaration)
] @local.scope

(parameter name: (identifier) @local.definition)
(lambda_parameter name: (identifier) @local.definition)
(lambda_expression parameters: (identifier) @local.definition)
(binding pattern: (identifier) @local.definition)
(let_declaration pattern: (identifier) @local.definition)
(variable_declaration name: (identifier) @local.definition)
(tuple_pattern (identifier) @local.definition)
(list_pattern (identifier) @local.definition)
(rest_pattern (identifier) @local.definition)
(field_pattern pattern: (identifier) @local.definition)
(constructor_pattern (identifier) @local.definition)
(for_statement pattern: (identifier) @local.definition)
(for_clause pattern: (identifier) @local.definition)

(identifier) @local.reference
