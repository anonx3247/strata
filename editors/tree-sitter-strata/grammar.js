/**
 * @file Strata grammar for tree-sitter
 * @license MIT
 *
 * It follows Strata's own parser (`compiler/parse.strata`, read with
 * `strata --file compiler view parse`): the same precedence levels, the same
 * contextual words. Where Strata looks at the layout, the external scanner
 * (`src/scanner.c`) does:
 *
 * - a call's `(` is written against what it calls (`f(x)`; `f (x)` is no
 *   call), so a line starting with `(` begins a new statement;
 * - an index's `[`, a binary `-`, and a postfix `!` or `?` are on the line of
 *   what they follow.
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

const PREC = {
  // `x -> body`, `return e`, `spawn e`: they take the whole expression
  lambda: -1,
  implies: 1,
  range: 2,
  or: 3,
  and: 4,
  compare: 5,
  fallback: 6,
  bitor: 7,
  bitxor: 8,
  bitand: 9,
  shift: 10,
  additive: 11,
  multiplicative: 12,
  unary: 13,
  // `!`, `?`, `except`, `or { }` after a value
  handler: 14,
  // `await h`: the handlers after it are the result's
  await: 15,
  postfix: 16,
};

/** @param {RuleOrLiteral} rule */
const commaSep = (rule) => optional(seq(rule, repeat(seq(',', rule)), optional(',')));

/** @param {RuleOrLiteral} rule */
const commaSep1 = (rule) => seq(rule, repeat(seq(',', rule)), optional(','));

/**
 * @param {RuleOrLiteral} rule
 * @param {RuleOrLiteral} sep
 */
const sep1 = (rule, sep) => seq(rule, repeat(seq(sep, rule)));

module.exports = grammar({
  name: 'strata',

  word: ($) => $.identifier,

  extras: ($) => [/\s/, $.comment, $.doc_comment],

  externals: ($) => [
    $._call_lparen,
    $._index_lbracket,
    $._try_bang,
    $._try_question,
    $._binary_minus,
    // never produced: tells the scanner it is called in error recovery
    $._error_sentinel,
  ],

  supertypes: ($) => [$._declaration, $._expression, $._pattern, $._type],

  conflicts: ($) => [
    [$._primary, $.select_arm],
    [$._type_name, $.scoped_type_identifier],
    [$.field_initializer, $._field_name],
    [$._type, $.union_type],
    [$._type, $.refined_type],
    [$._primary, $.negative_integer],
    [$.lambda_parameter, $.wildcard_pattern],
    // `(a, b)`: a tuple, a lambda's parameters, or a pattern (`(a, b) := e`)
    [$._primary, $.lambda_parameter],
    [$._primary, $._pattern_one],
    [$._primary, $._pattern_one, $.lambda_parameter],
    [$.unit, $.tuple_pattern],
    [$.list_expression, $.list_pattern],
    [$._primary, $.constructor_pattern],
    [$._primary, $.record_expression, $.constructor_pattern],
    [$._primary, $.record_expression],
    [$.scoped_type_identifier, $._primary],
    [$.record_fields, $.record_pattern],
  ],

  rules: {
    source_file: ($) => repeat($._declaration),

    // ---------- declarations ----------

    _declaration: ($) =>
      choice(
        $.use_declaration,
        $.import_declaration,
        $.type_declaration,
        $.mock_declaration,
        $.function_declaration,
        $.trait_declaration,
        $.impl_declaration,
        $.derive_declaration,
        $.capability_declaration,
        $.extern_declaration,
        $.builtin_declaration,
        $.test_declaration,
      ),

    visibility_modifier: (_) => 'pub',

    // `use std.time`, `use std.time as t`
    use_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        'use',
        field('path', $.module_path),
        optional(seq('as', field('alias', $.identifier))),
      ),

    // `from std.time use Clock, Timer as T`
    import_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        'from',
        field('path', $.module_path),
        'use',
        commaSep1($.import_item),
      ),

    import_item: ($) =>
      seq(
        field('name', $._name),
        optional(seq('as', field('alias', $._name))),
      ),

    module_path: ($) => sep1($.identifier, '.'),

    // `type Point = { x: Int, y: Int }`, `type Shape = | Circle(Float) | Dot`,
    // `distinct type Port = Int where it > 0`, `type Moved(Int)`
    type_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        optional('distinct'),
        'type',
        field('name', $.type_identifier),
        optional(field('type_parameters', $.type_parameters)),
        optional(
          choice(
            seq(
              '=',
              field('body', choice($.record_body, $.union_body)),
              optional($.refinement),
            ),
            field('payload', $.tuple_payload),
          ),
        ),
      ),

    // `mock MemFiles = { files: Dict[String, String] }`
    mock_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        'mock',
        field('name', $.type_identifier),
        optional(field('type_parameters', $.type_parameters)),
        '=',
        field('body', $.record_body),
      ),

    union_body: ($) => seq(optional('|'), sep1($.variant, '|')),

    // `Circle(Float)`, `Square { side: Float }`, or a type: `Dot`, `List[Int]`
    variant: ($) =>
      choice(
        seq(
          field('name', $.type_identifier),
          field('payload', choice($.tuple_payload, $.record_body)),
        ),
        field('type', $._type_plain),
      ),

    tuple_payload: ($) => seq('(', commaSep($._type), ')'),

    record_body: ($) => seq('{', commaSep($.field_declaration), '}'),

    field_declaration: ($) =>
      seq(
        field('name', $._field_name),
        ':',
        field('type', $._type),
        optional(seq('=', field('default', $._expression))),
      ),

    refinement: ($) => seq('where', $._expression),

    // `fn area(s: Shape) -> Float requires … { … }`; a trait's method, or
    // an `extern`/`builtin` one, has no body
    function_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        optional(choice('comptime', 'derived')),
        'fn',
        $._function_signature,
        optional($._function_body),
      ),

    // (`prec.right`: an `ensures` after a method without a body is the
    // method's, as `strata` reads it)
    _function_signature: ($) =>
      prec.right(seq(
        field('name', $.identifier),
        optional(field('type_parameters', $.type_parameters)),
        field('parameters', $.parameters),
        optional(seq('->', field('return_type', choice($._type, $.named_results)))),
        repeat($.contract),
      )),

    _function_body: ($) =>
      choice(field('body', $.block), seq('=', field('body', $._expression))),

    contract: ($) =>
      choice(
        'terminates',
        seq(optional('assumed'), 'requires', $._expression),
        seq(optional('assumed'), 'ensures', $._expression),
        seq('decreases', $._expression),
      ),

    // `-> (next: Queue, added: Int)`: the result's elements, named
    named_results: ($) =>
      seq('(', commaSep1($.named_result), ')'),

    named_result: ($) =>
      seq(field('name', $.identifier), ':', field('type', $._type)),

    // `[T: Ord + Hash, U]`
    type_parameters: ($) => seq('[', commaSep($.type_parameter), ']'),

    type_parameter: ($) =>
      seq(
        field('name', $.type_identifier),
        optional(seq(':', field('bounds', $.trait_bounds))),
      ),

    trait_bounds: ($) => sep1($._type_plain, '+'),

    parameters: ($) => seq('(', commaSep($.parameter), ')'),

    // `x: Int`, `mut s: State`, `comptime t: Type`, `self`
    parameter: ($) =>
      seq(
        optional('comptime'),
        optional('mut'),
        choice(
          seq(
            field('name', choice($.identifier, $.self)),
            ':',
            field('type', $._type),
          ),
          field('name', $.self),
        ),
      ),

    self: (_) => 'self',

    // `trait Show { fn show(self) -> String }`, `capability trait Db { … }`,
    // `derivable(Record) trait Json requires … { … }`
    trait_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        optional(
          choice('capability', seq('derivable', '(', field('kind', $._type), ')')),
        ),
        'trait',
        field('name', $.type_identifier),
        optional(field('type_parameters', $.type_parameters)),
        optional(seq(':', field('supertraits', $.trait_bounds))),
        repeat(seq('requires', $._expression)),
        field('body', $.trait_body),
      ),

    trait_body: ($) =>
      seq(
        '{',
        repeat(
          choice(
            $.model_declaration,
            $.trait_invariant,
            $.trait_ensures,
            $.function_declaration,
            $.law_declaration,
          ),
        ),
        '}',
      ),

    // `model orders: Dict[Id, Order]`: a trait's abstract state
    model_declaration: ($) =>
      seq('model', field('name', $.identifier), ':', field('type', $._type)),

    // `invariant self.count >= 0`, `ensures self.count >= old self.count - 1`
    // in a trait: what every method keeps, what every `mut self` one ensures
    trait_invariant: ($) => seq(optional('assumed'), 'invariant', $._expression),
    trait_ensures: ($) => seq(optional('assumed'), 'ensures', $._expression),

    // `model orders = self.all` in an `impl`
    model_definition: ($) =>
      seq('model', field('name', $.identifier), '=', field('value', $._expression)),

    // `law reflexive(a: Self): a == a`
    law_declaration: ($) =>
      seq(
        'law',
        field('name', $.identifier),
        field('parameters', $.parameters),
        ':',
        field('body', $._expression),
      ),

    // `impl Show for Point { … }`, `impl Point { … }`, `impl[T] Show for List[T]`
    impl_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        'impl',
        optional(field('type_parameters', $.type_parameters)),
        field('trait', $._type_plain),
        optional(seq('for', field('type', $._type_plain))),
        field('body', $.impl_body),
      ),

    // `derive Json for Point`, with `{ … }` for methods written by hand
    derive_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        'derive',
        optional(field('type_parameters', $.type_parameters)),
        field('trait', $._type_plain),
        'for',
        field('type', $._type_plain),
        optional(field('body', $.impl_body)),
      ),

    impl_body: ($) =>
      seq('{', repeat(choice($.model_definition, $.function_declaration)), '}'),

    // `capability Sqlite { fn open(…) … }`
    capability_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        'capability',
        field('name', $.type_identifier),
        field('body', $.impl_body),
      ),

    // `extern c fn strlen(s: Ptr) -> Int`, `extern c type File`
    extern_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        'extern',
        field('language', $.identifier),
        choice(
          seq('fn', $._function_signature),
          seq('type', field('name', $.type_identifier)),
        ),
      ),

    // `builtin fn len[A](xs: List[A]) -> Int`, `builtin type List[T]`,
    // `builtin static allocator: 512`, `builtin thread static calls: 64`
    builtin_declaration: ($) =>
      seq(
        optional($.visibility_modifier),
        'builtin',
        choice(
          seq('fn', $._function_signature),
          seq(
            'type',
            field('name', $.type_identifier),
            optional(field('type_parameters', $.type_parameters)),
          ),
          seq(
            optional('thread'),
            'static',
            field('name', $.identifier),
            ':',
            field('size', $.integer),
          ),
        ),
      ),

    // `test "adds" { assert 1 + 1 == 2 }`, `test "reads" (mut files: Files) { … }`
    test_declaration: ($) =>
      seq(
        'test',
        field('name', $.string),
        optional(field('parameters', $.parameters)),
        field('body', $.block),
      ),

    // ---------- types ----------

    _type: ($) => choice($.union_type, $.refined_type, $._type_plain),

    union_type: ($) =>
      prec.left(seq($._type_plain, repeat1(seq('|', $._type_plain)))),

    // `Int where it > 0`
    refined_type: ($) =>
      seq(
        field('base', choice($.union_type, $._type_plain)),
        'where',
        field('predicate', $._expression),
      ),

    _type_plain: ($) =>
      choice(
        $.generic_type,
        $._type_name,
        $.function_type,
        $.tuple_type,
      ),

    _type_name: ($) =>
      choice($.type_identifier, $.scoped_type_identifier, $.identifier),

    // `geo.Circle`, `f.type`
    scoped_type_identifier: ($) =>
      prec.left(
        seq(
          field('path', choice($.identifier, $.scoped_type_identifier)),
          '.',
          field('name', choice($.type_identifier, $.identifier, alias('type', $.identifier))),
        ),
      ),

    generic_type: ($) =>
      prec(1, seq(field('name', $._type_name), field('arguments', $.type_arguments))),

    type_arguments: ($) => seq('[', commaSep($._type), ']'),

    // `(Int, String) -> Bool`
    function_type: ($) =>
      prec.right(
        seq(
          field('parameters', $.tuple_type),
          '->',
          field('return_type', $._type),
        ),
      ),

    // `()`, `(A, B)`, `(A)`
    tuple_type: ($) => seq('(', commaSep($._type), ')'),

    // ---------- statements ----------

    block: ($) => seq('{', repeat(choice($._statement, ';')), '}'),

    _statement: ($) =>
      choice(
        $.let_declaration,
        $.variable_declaration,
        $.binding,
        $.assignment,
        $.assert_statement,
        $.loop_statement,
        $.while_statement,
        $.for_statement,
        $.comptime_for_statement,
        $.break_statement,
        $.continue_statement,
        $._expression,
      ),

    // `let p: Point = e`
    let_declaration: ($) =>
      seq(
        'let',
        field('pattern', $._pattern),
        ':',
        field('type', $._type),
        '=',
        field('value', $._expression),
      ),

    // `mut x := e`, `mut x: T = e`
    variable_declaration: ($) =>
      seq(
        'mut',
        field('name', $.identifier),
        choice(
          seq(':=', field('value', $._expression)),
          seq(':', field('type', $._type), '=', field('value', $._expression)),
        ),
      ),

    // `x := e`, `(a, b) := e`
    binding: ($) =>
      seq(field('pattern', $._pattern), ':=', field('value', $._expression)),

    // `x = e`, `o.total += n`
    assignment: ($) =>
      seq(
        field('left', $._expression),
        field('operator', choice('=', '+=', '-=', '*=', '/=', '%=')),
        field('right', $._expression),
      ),

    assert_statement: ($) => seq('assert', $._expression),

    break_statement: (_) => 'break',

    continue_statement: (_) => 'continue',

    // `assumed invariant e`: trusted (proven by verify or assumed), never checked at run time
    invariant: ($) => seq(optional('assumed'), 'invariant', $._expression),

    // `decreases n - i`, `decreases (a, b)`: why a loop ends
    decreases: ($) => seq('decreases', $._expression),

    _loop_clause: ($) => choice($.invariant, $.decreases),

    loop_statement: ($) =>
      seq('loop', repeat($._loop_clause), field('body', $.block)),

    while_statement: ($) =>
      seq(
        'while',
        field('condition', $._expression),
        repeat($._loop_clause),
        field('body', $.block),
      ),

    for_statement: ($) =>
      seq(
        'for',
        field('pattern', $._pattern),
        'in',
        field('iterable', $._expression),
        repeat($._loop_clause),
        field('body', $.block),
      ),

    comptime_for_statement: ($) =>
      seq(
        'comptime',
        'for',
        field('pattern', $._pattern),
        'in',
        field('iterable', $._expression),
        field('body', $.block),
      ),

    // ---------- expressions ----------

    _expression: ($) =>
      choice(
        $.binary_expression,
        $.unary_expression,
        $.range_expression,
        $.is_expression,
        $.lambda_expression,
        $.return_expression,
        $.spawn_expression,
        $.await_expression,
        $.comptime_expression,
        $.old_expression,
        $.mut_expression,
        $.try_expression,
        $.except_expression,
        $.or_expression,
        $.call_expression,
        $.field_expression,
        $.index_expression,
        $._primary,
      ),

    _primary: ($) =>
      choice(
        $.identifier,
        $.type_identifier,
        $.integer,
        $.float,
        $.string,
        $.f_string,
        $.hole,
        $.boolean,
        $.unit,
        $.parenthesized_expression,
        $.tuple_expression,
        $.list_expression,
        $.list_comprehension,
        $.dict_expression,
        $.set_expression,
        $.record_expression,
        $.block,
        $.if_expression,
        $.match_expression,
        $.select_expression,
        $.scope_expression,
        $.mock_expression,
      ),

    binary_expression: ($) => {
      /** @type {[number, RuleOrLiteral][]} */
      const table = [
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.compare, choice('==', '!=', '<', '<=', '>', '>=')],
        [PREC.bitor, '|'],
        [PREC.bitxor, '^'],
        [PREC.bitand, '&'],
        [PREC.shift, choice('<<', '>>')],
        [PREC.additive, choice('+', alias($._binary_minus, '-'))],
        [PREC.multiplicative, choice('*', '/', '%')],
      ];
      return choice(
        ...table.map(([p, op]) =>
          prec.left(
            p,
            seq(
              field('left', $._expression),
              field('operator', op),
              field('right', $._expression),
            ),
          ),
        ),
        // `a ?? b`, `a !! b`
        prec.right(
          PREC.fallback,
          seq(
            field('left', $._expression),
            field('operator', choice('??', '!!')),
            field('right', $._expression),
          ),
        ),
        // `a ==> b`
        prec.right(
          PREC.implies,
          seq(
            field('left', $._expression),
            field('operator', '==>'),
            field('right', $._expression),
          ),
        ),
      );
    },

    unary_expression: ($) =>
      prec(
        PREC.unary,
        seq(field('operator', choice('-', '!', '!!')), field('operand', $._expression)),
      ),

    // `0..n`, `1..=n`
    range_expression: ($) =>
      prec.left(
        PREC.range,
        seq(
          field('start', $._expression),
          field('operator', choice('..', '..=')),
          field('end', $._expression),
        ),
      ),

    // `s is Circle`
    is_expression: ($) =>
      prec.left(
        PREC.compare,
        seq(field('value', $._expression), 'is', field('variant', $._type_name)),
      ),

    // `x -> x + 1`, `(a, b: Int) -> a + b`, `() -> 0`
    lambda_expression: ($) =>
      prec.right(
        PREC.lambda,
        seq(
          field(
            'parameters',
            choice($.identifier, alias('_', $.identifier), $.lambda_parameters),
          ),
          '->',
          field('body', $._expression),
        ),
      ),

    lambda_parameters: ($) => seq('(', commaSep($.lambda_parameter), ')'),

    lambda_parameter: ($) =>
      seq(
        field('name', choice($.identifier, alias('_', $.identifier))),
        optional(seq(':', field('type', $._type))),
      ),

    return_expression: ($) =>
      prec.right(PREC.lambda, seq('return', optional($._expression))),

    // `spawn work(x)`, `supervised spawn work(x)`
    spawn_expression: ($) =>
      prec.right(
        PREC.lambda,
        seq(choice('spawn', $.supervised_spawn), $._expression),
      ),

    await_expression: ($) => prec(PREC.await, seq('await', $._expression)),

    comptime_expression: ($) =>
      prec(PREC.unary, seq('comptime', $._expression)),

    // `old len(xs)`: in an `ensures`, the value where the function started
    old_expression: ($) => prec(PREC.unary, seq('old', $._expression)),

    // `f(mut x)`, `mut q.claim(2)`
    mut_expression: ($) => prec(PREC.unary, seq('mut', $._expression)),

    // `read(path)!`, `get(xs, 0)?`
    try_expression: ($) =>
      prec(
        PREC.handler,
        seq(
          field('value', $._expression),
          field('operator', choice(alias($._try_bang, '!'), alias($._try_question, '?'))),
        ),
      ),

    // `read(path) except e { … }`
    except_expression: ($) =>
      prec(
        PREC.handler,
        seq(
          field('value', $._expression),
          'except',
          optional(field('error', $.identifier)),
          field('handler', $.block),
        ),
      ),

    // `get(xs, 0) or { 0 }`
    or_expression: ($) =>
      prec(
        PREC.handler,
        seq(field('value', $._expression), 'or', field('handler', $.block)),
      ),

    call_expression: ($) =>
      prec(
        PREC.postfix,
        seq(field('function', $._expression), field('arguments', $.arguments)),
      ),

    arguments: ($) =>
      seq(alias($._call_lparen, '('), commaSep($._expression), ')'),

    // `p.x`, `pair.0`, `f.type`, `shop.Pending`
    field_expression: ($) =>
      prec(
        PREC.postfix,
        seq(
          field('value', $._expression),
          '.',
          field(
            'field',
            choice(
              alias($.identifier, $.field_identifier),
              $.type_identifier,
              alias($.integer, $.field_identifier),
              alias('type', $.field_identifier),
            ),
          ),
        ),
      ),

    // `xs[i]`, `xs[a..b]`, `xs[..n]`, `xs[a..]`
    index_expression: ($) =>
      prec(
        PREC.postfix,
        seq(
          field('value', $._expression),
          alias($._index_lbracket, '['),
          field('index', choice($._expression, $.slice)),
          ']',
        ),
      ),

    slice: ($) =>
      choice(
        seq(choice('..', '..='), optional($._expression)),
        seq($._expression, '..'),
      ),

    unit: (_) => seq('(', ')'),

    parenthesized_expression: ($) => seq('(', $._expression, ')'),

    tuple_expression: ($) =>
      seq('(', $._expression, ',', commaSep($._expression), ')'),

    list_expression: ($) => seq('[', commaSep($._expression), ']'),

    // `[x * 2 for x in xs where x > 0]`
    list_comprehension: ($) =>
      seq('[', field('element', $._expression), repeat1($.for_clause), ']'),

    for_clause: ($) =>
      seq(
        'for',
        field('pattern', $._pattern),
        'in',
        field('iterable', $._expression),
        optional(seq('where', field('condition', $._expression))),
      ),

    // `Dict { "a": 1 }`, `Dict[String, Int] {}`, `Dict { k: v for … }`
    dict_expression: ($) =>
      seq(
        'Dict',
        optional($._collection_types),
        '{',
        choice(
          commaSep($.pair),
          seq(field('element', $.pair), repeat1($.for_clause)),
        ),
        '}',
      ),

    pair: ($) =>
      seq(field('key', $._expression), ':', field('value', $._expression)),

    // `Set { 1, 2 }`, `Set[Int] {}`, `Set { x % 3 for x in xs }`
    set_expression: ($) =>
      seq(
        'Set',
        optional($._collection_types),
        '{',
        choice(
          commaSep($._expression),
          seq(field('element', $._expression), repeat1($.for_clause)),
        ),
        '}',
      ),

    _collection_types: ($) =>
      seq(alias($._index_lbracket, '['), commaSep($._type), ']'),

    // `Point { x: 1, y }`, `Point { ..p, x: 2 }`, `shop.Line { … }`
    // (after `if x ==` a `Red {` may be the block: both are tried)
    record_expression: ($) =>
      prec.dynamic(
        -1,
        seq(
          field('name', choice($.type_identifier, $.scoped_type_identifier)),
          field('fields', $.record_fields),
        ),
      ),

    record_fields: ($) =>
      seq(
        '{',
        optional(
          choice(
            seq($.base_record, optional(seq(',', commaSep($.field_initializer)))),
            commaSep1($.field_initializer),
          ),
        ),
        '}',
      ),

    base_record: ($) => seq('..', $._expression),

    field_initializer: ($) =>
      choice(
        seq(field('name', $._field_name), ':', field('value', $._expression)),
        field('name', $.identifier),
      ),

    if_expression: ($) =>
      prec.right(
        seq(
          'if',
          field('condition', $._expression),
          field('consequence', $.block),
          optional(
            seq('else', field('alternative', choice($.if_expression, $.block))),
          ),
        ),
      ),

    match_expression: ($) =>
      seq(
        'match',
        field('value', $._expression),
        optional(seq('as', field('binding', $.identifier))),
        '{',
        repeat($.match_arm),
        '}',
      ),

    // `Circle(r) => 3.14 * r * r,`; `v => e for v in Self.variants`
    match_arm: ($) =>
      prec.right(
        seq(
          field('pattern', $._pattern),
          '=>',
          field('value', $._expression),
          optional(
            seq(
              'for',
              field('each', $._pattern),
              'in',
              field('list', $._expression),
            ),
          ),
          optional(','),
        ),
      ),

    // `select { rx.receive() as m => …, }`, `race { … }`
    select_expression: ($) =>
      seq(choice('select', 'race'), '{', repeat($.select_arm), '}'),

    select_arm: ($) =>
      prec.right(
        seq(
          // a race arm's `supervised op`: the word is a name everywhere else
          optional(field('supervised', $.identifier)),
          field('operation', $._expression),
          optional(seq('as', field('name', $.identifier))),
          '=>',
          field('value', $._expression),
          optional(','),
        ),
      ),

    // `supervised` is a word only right before `spawn`: elsewhere it is a name
    supervised_spawn: (_) => token(prec(1, /supervised[ \t]+spawn/)),

    scope_expression: ($) => seq('scope', field('body', $.block)),

    mock_expression: ($) => seq('mock', field('type', $._type_plain)),

    // ---------- patterns ----------

    _pattern: ($) => choice($.or_pattern, $._pattern_one),

    or_pattern: ($) =>
      prec.left(seq($._pattern_one, repeat1(seq('|', $._pattern_one)))),

    _pattern_one: ($) =>
      choice(
        $.wildcard_pattern,
        $.where_pattern,
        $.boolean,
        $.integer,
        $.negative_integer,
        $.string,
        $.tuple_pattern,
        $.list_pattern,
        $.identifier,
        $.constructor_pattern,
      ),

    wildcard_pattern: (_) => '_',

    // `where it > 0`
    where_pattern: ($) => prec.right(seq('where', $._expression)),

    negative_integer: ($) => seq('-', $.integer),

    tuple_pattern: ($) => seq('(', commaSep($._pattern), ')'),

    // `[first, ..rest]`
    list_pattern: ($) =>
      seq('[', commaSep(choice($._pattern, $.rest_pattern)), ']'),

    rest_pattern: ($) => seq('..', optional($.identifier)),

    // `Circle(r)`, `Square { side }`, `Option[t]`, `shop.Pending`
    constructor_pattern: ($) =>
      prec.right(
        seq(
          field('name', choice($.type_identifier, $.scoped_type_identifier)),
          optional(
            choice(
              seq(
                choice('(', alias($._call_lparen, '(')),
                commaSep($._pattern),
                ')',
              ),
              seq(alias($._index_lbracket, '['), commaSep($._pattern), ']'),
              $.record_pattern,
            ),
          ),
        ),
      ),

    record_pattern: ($) =>
      seq('{', commaSep(choice($.field_pattern, '..')), '}'),

    field_pattern: ($) =>
      seq(
        field('name', $._field_name),
        optional(seq(':', field('pattern', $._pattern))),
      ),

    // ---------- names and literals ----------

    _name: ($) => choice($.identifier, $.type_identifier),

    _field_name: ($) =>
      choice(alias($.identifier, $.field_identifier), alias('type', $.field_identifier)),

    // a name starting with a lowercase letter or `_`: values, functions, modules
    identifier: (_) => /[\p{Ll}\p{Lm}\p{Lo}_][\p{L}\p{M}\p{N}_]*/,

    // a name starting with an uppercase letter: types and constructors
    type_identifier: (_) => /[\p{Lu}\p{Lt}][\p{L}\p{M}\p{N}_]*/,

    integer: (_) => /\d[\d_]*/,

    float: (_) => /\d[\d_]*\.\d[\d_]*/,

    boolean: (_) => choice('true', 'false'),

    // `?todo`: a hole to fill in
    hole: (_) => /\?\p{L}[\p{L}\p{M}\p{N}_]*/,

    string: ($) =>
      seq(
        '"',
        repeat(choice($.string_content, $.escape_sequence)),
        token.immediate('"'),
      ),

    string_content: (_) => token.immediate(prec(3, /[^"\\]+/)),

    escape_sequence: (_) => token.immediate(/\\(.|\n)/),

    // `f"{name} has {len(xs)} items, {price:.2}"`
    f_string: ($) =>
      seq(
        'f"',
        repeat(
          choice(
            alias($._f_string_content, $.string_content),
            $.escape_sequence,
            alias(token.immediate('{{'), $.escape_sequence),
            alias(token.immediate('}}'), $.escape_sequence),
            $.interpolation,
          ),
        ),
        token.immediate('"'),
      ),

    _f_string_content: (_) => token.immediate(prec(3, /[^"\\{}]+/)),

    interpolation: ($) =>
      seq(
        token.immediate('{'),
        field('value', $._expression),
        optional(seq(':', field('format', $.format_spec))),
        '}',
      ),

    format_spec: (_) => token.immediate(/[^}]+/),

    comment: (_) => token(seq('//', /[^\n]*/)),

    doc_comment: (_) => token(prec(1, seq('///', /[^\n]*/))),
  },
});
