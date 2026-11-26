---
title: JSX-like pages
---

# Why
Pages in Strata were built with one function per element: `td([class("num")], [text(to_string(l.year))])`. The user asked for pages written like JSX: `<td class="num">{l.year}</td>`, with text escaped by construction, components as plain functions, and the page rendered to a string on the server. REBUILD rule 7 says sugar lives only in the parser and the printer, so the stored tree stays the calls.

# Background
`std.web` already had the values and the rendering: an `Html[M]` is an element or text, `element` checks the tag and refuses children for void elements, and `render` escapes text and attribute values.

```strata std.web.element
```

```strata std.web.render
```

The lexer turns text into tokens, `parse.primary_inner` reads an operand from them, and `text.expr_text` prints an expression back, breaking it where it is too long.

```strata parse.primary_inner
```

# Intuition
`<p class="x">Hi {name}</p>` is read as `std.web.element("p", [std.web.attr("class", "x")], std.web.children(["Hi ", name]))`. `children` takes a list of `Child` values (an element, a string, a number or a list of elements) and flattens them, so `{name}`, `{row(l)}` and `{ls.map(row)}` need no syntax of their own: the types decide. `view` sees those calls and prints the markup again.

```strata-refs std.web.children
```

# The lexer reads markup
A `<` is markup when a letter or `>` follows at once and it stands where an operand goes (`markup_starts`): `x := <b>` is markup, `a <b` compares. A stack of modes says where the lexer is: in a start tag (attribute names with dashes, strings, `{`), in children (text up to the next `<` or `{`, read as JSX reads it), or in a `{…}`, which is lexed as code until its braces close, so markup nests inside code inside markup. End tags are matched there, so a wrong `</div>` or a `<br>` without `/>` is an error at its place.

```strata-diff lex.lex lex.Mode lex.markup_children lex.markup_text
```

```strata-also
lex.Tok: the markup tokens
lex.Lexer: the mode stack
lex.markup_starts lex.ends_operand: where markup starts
lex.markup_open lex.markup_tag: start tags and attributes
lex.string: string literals, out of `lex` so start tags read them too
lex.mode lex.push_mode lex.pop_mode lex.set_mode lex.code_brace lex.in_children lex.in_tag: the mode stack
lex.is_ascii_letter lex.is_tag_char lex.is_void_tag lex.trim_start: helpers
lex.token_text: the new tokens in `strata lex`
```

# The parser makes calls
`markup` reads an element into `std.web` calls: `name="x"` is `attr`, `name={e}` is `attribute` (a `String`, `Int` or `Bool`), a bare name is `flag`, `on_click={m}` is `on_click(m)`. A capitalized tag and `onclick` are refused with what to write instead.

```strata-diff parse.markup parse.markup_attr parse.markup_children
```

```strata-also
parse.primary_inner: markup is an operand
parse.markup_code parse.web_call parse.void_tag: helpers
parse.tok_shown: the new tokens in messages
```

# The printer shows markup again
`expr_text` asks `markup_view` first. Only the calls the parser makes are shown as markup (a literal tag, attributes from `attr`, `attribute`, `flag` or an `on_` event); anything else stays a call, so reading the view back gives the same tree. Children fill lines; text whose edge spaces a line break would drop is written `{" more"}`.

```strata-diff text.markup_view text.markup_parts text.markup_lines text.markup_child
```

```strata-also
text.expr_text: calls `markup_view`; a list or call holding markup breaks when the markup does
text.holds_markup text.markup_line text.markup_children_line text.markup_attr_view text.web_fn text.is_void_tag: helpers
```

# std.web: children, attribute values, raw HTML
`children` flattens the children, `attribute` takes a `String`, an `Int`, or a `Bool` (a flag, left out when `false`: the new `Absent` kind). `raw_html` is the one way to write HTML unescaped.

```strata-diff std.web.Child std.web.children std.web.attribute std.web.raw_html
```

```strata-also
std.web.AttrValue: what `name={e}` takes
std.web.Node std.web.AttrKind: the `Raw` node and the `Absent` attribute
std.web.render_from std.web.render_attr std.web.handlers: rendering them
std.web.TagName: the module's top comment, which is stored on it
```

# Codegen: unions whose members have type arguments
`children` matches `List[Html[M]] =>` and `Html[M] =>`. A pattern with type arguments is now always a type test (`type_pattern`). And a boxed union member is tagged by its head alone (`List`) when no other member has that head (`box_tag_in`): the caller boxed `xs.map(f)` for `Child[?]` (the call's type argument still open) while the instance tested for `Child[()]`, two different tags, so the list matched no arm.

```strata-diff codegen.type_pattern codegen.box_tag_in codegen.to_union
```

```strata-also
codegen.member_tags codegen.holds_member codegen.read_slot: tags through `box_tag_in`
```

# The example
`examples/page` is written with markup and is proven (`verify --require proven`); its tests check escaping in text and attributes, `raw_html`, numbers, flags and lists in braces.

```strata-diff row languages_table sort_link page
```

```strata-also
test*: the page's tests in markup, and new ones for escaping, raw HTML, flags and lists
use*: the names the page imports
```
