# std.web

HTML pages built as values, one function per element, rendered by `render` or `document`.
A page with events has a message type `M`. A page without events is an `Html[()]`.

## Types

### TagName

```rust
type TagName = String
  where matches(it, "[a-z][a-z0-9]*(-[a-z0-9]+)*")
```

Lowercase letters, digits and dashes: `div`, `my-card`.

### AttrName

```rust
type AttrName = String
  where matches(it, "[a-zA-Z_:][-a-zA-Z0-9_:.]*")
    && !matches(it, "[oO][nN].*")
```

An attribute's name. Names starting with `on` are inline scripts, which pages can't hold. Use `on(event, msg)` instead.

### EventName

```rust
type EventName = String
  where matches(it, "[a-z]+")
```

A DOM event's name without `on`: `click`, `submit`, `input`.

### Html

```rust
type Html[M] = { node: Node[M] }
```

HTML whose events carry messages of type `M`.
Only this module's functions make one, so names are checked once, where it is made (`element`, `attr`).
Text is escaped when rendered, so it can't inject markup.

### Attr

```rust
type Attr[M] = { kind: AttrKind[M] }
```

An attribute of an element. Made by `attr`, `flag` and `on`.

### KeyName

```rust
type KeyName = String
  where matches(it, "(ctrl\+)?(alt\+)?(meta\+)?.+")
```

A key for `on_key`: what the key types (`a`, `/`, `?`) or its name (`Escape`, `Enter`, `ArrowLeft`).
Modifiers come first, in this order: `ctrl+`, `alt+`, `meta+`. `meta+p` is ⌘P on a Mac.
Shift is not named. It is in what the key types (`?`, `A`).

## Functions

### escape_text

```rust
fn escape_text(s: String) -> String
```

`s` with `&`, `<` and `>` escaped, for text between tags.

### escape_attr

```rust
fn escape_attr(s: String) -> String
```

`s` escaped for an attribute value between double quotes.

### is_void

```rust
fn is_void(tag: String) -> Bool
```

Elements that have no children and no closing tag.

### render

```rust
fn render[M](h: Html[M]) -> String
```

`h` as HTML text.

### dispatch

```rust
fn dispatch[M](
  h: Html[M],
  id: Int,
  value: String,
  fields: Dict[String, String],
) -> Option[M]
```

The message of handler number `id` of the page `h`, or `None` if there is no such handler.
A handler's message is its own (`on`, `every`, `on_key`), or made from a field's `value` or a form's `fields`.
The runtime calls it with the page the user sees when a `data-handle-…` event comes.

### document

```rust
fn document[M](
  title_text: String,
  head_items: List[Html[M]],
  body_items: List[Html[M]],
) -> String
```

A whole page: the doctype, a `<head>` with the title and `head`, and a `<body>` holding `body`.

### element

```rust
fn element[M](
  tag: TagName,
  attrs: List[Attr[M]],
  children: List[Html[M]],
) -> Html[M]
  requires len(children) == 0
    || !matches(
      tag,
      "area|base|br|col|embed|hr|img|input|link|meta|source|track|wbr",
    )
```

Any element. Void elements (`br`, `img`, `input`, …) can't have children.
The condition is `is_void` written out, so the prover sees it.

### text

```rust
fn text[M](s: String) -> Html[M]
```

Text, escaped when rendered.

### style_sheet

```rust
fn style_sheet[M](css: String) -> Html[M]
```

A `<style>` element holding a style sheet.

### style_link

```rust
fn style_link[M](url: String) -> Html[M]
```

A `<link rel="stylesheet">` to the style sheet at `url`.

### title

```rust
fn title[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### div

```rust
fn div[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### span

```rust
fn span[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### p

```rust
fn p[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### a

```rust
fn a[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### h1

```rust
fn h1[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### h2

```rust
fn h2[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### h3

```rust
fn h3[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### strong

```rust
fn strong[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### em

```rust
fn em[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### code

```rust
fn code[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### pre

```rust
fn pre[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### ul

```rust
fn ul[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### ol

```rust
fn ol[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### li

```rust
fn li[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### table

```rust
fn table[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### thead

```rust
fn thead[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### tbody

```rust
fn tbody[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### tr

```rust
fn tr[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### th

```rust
fn th[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### td

```rust
fn td[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### header

```rust
fn header[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### footer

```rust
fn footer[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### nav

```rust
fn nav[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### section

```rust
fn section[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### article

```rust
fn article[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### form

```rust
fn form[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### label

```rust
fn label[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### button

```rust
fn button[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### textarea

```rust
fn textarea[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### select

```rust
fn select[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### option

```rust
fn option[M](attrs: List[Attr[M]], children: List[Html[M]]) -> Html[M]
```

### input

```rust
fn input[M](attrs: List[Attr[M]]) -> Html[M]
```

An `<input>`. A void element, so attributes only.

### img

```rust
fn img[M](attrs: List[Attr[M]]) -> Html[M]
```

An `<img>`. A void element, so attributes only.

### br

```rust
fn br[M]() -> Html[M]
```

A line break.

### hr

```rust
fn hr[M]() -> Html[M]
```

A horizontal rule.

### attr

```rust
fn attr[M](name: AttrName, value: String) -> Attr[M]
```

Any attribute: `name="value"`.

### flag

```rust
fn flag[M](name: AttrName) -> Attr[M]
```

An attribute without a value, such as `required`.

### class

```rust
fn class[M](names: String) -> Attr[M]
```

### id

```rust
fn id[M](value: String) -> Attr[M]
```

### href

```rust
fn href[M](url: String) -> Attr[M]
```

### src

```rust
fn src[M](url: String) -> Attr[M]
```

### alt

```rust
fn alt[M](text: String) -> Attr[M]
```

### style

```rust
fn style[M](css: String) -> Attr[M]
```

### input_type

```rust
fn input_type[M](kind: String) -> Attr[M]
```

The `type` attribute of an `input` or a `button`. `type` is a keyword.

### name

```rust
fn name[M](value: String) -> Attr[M]
```

### value

```rust
fn value[M](v: String) -> Attr[M]
```

### placeholder

```rust
fn placeholder[M](text: String) -> Attr[M]
```

### label_for

```rust
fn label_for[M](input_id: String) -> Attr[M]
```

The `for` attribute of a `label`. `for` is a keyword.

### disabled

```rust
fn disabled[M]() -> Attr[M]
```

### checked

```rust
fn checked[M]() -> Attr[M]
```

### required

```rust
fn required[M]() -> Attr[M]
```

### on

```rust
fn on[M](event: EventName, msg: M) -> Attr[M]
```

Sends `msg` to the program when `event` happens on the element.

### on_click

```rust
fn on_click[M](msg: M) -> Attr[M]
```

Sends `msg` when the element is clicked.

### on_submit

```rust
fn on_submit[M](msg: M) -> Attr[M]
```

Sends `msg` when the form is submitted.

### on_input

```rust
fn on_input[M](make: (String) -> M) -> Attr[M]
```

Sends `make(value)` as the user types in a field (`input`, `textarea`): `on_input(s -> SetTitle { title: s })`.

### on_change

```rust
fn on_change[M](make: (String) -> M) -> Attr[M]
```

Sends `make(value)` when the field's value changes (`select`, checkbox). A checkbox's value is `"true"` or `"false"`.

### on_submit_form

```rust
fn on_submit_form[M](make: (Dict[String, String]) -> M) -> Attr[M]
```

Sends `make(fields)` when the form is submitted, with its fields by name: `on_submit_form(fields -> Add { fields })`.
An unchecked checkbox is not among them.

### every

```rust
fn every[M](ms: Int where it >= 100, msg: M) -> Attr[M]
```

Sends `msg` every `ms` milliseconds while the element is on the page: `div([every(1500, Refresh)], [...])`.
A message isn't sent while the previous one is still being handled, so a slow `update` doesn't pile them up.

### on_key

```rust
fn on_key[M](key: KeyName, msg: M) -> Attr[M]
```

Sends `msg` when `key` is pressed anywhere on the page while the element is on it. The browser doesn't also act on the key.
While the user types in a field, only keys with `ctrl+` or `meta+`, and `Escape`, are sent.
One key per element. For several, use `key_bindings`.

### key_bindings

```rust
fn key_bindings[M](bindings: List[(KeyName, M)]) -> Html[M]
```

Several keys and their messages (see `on_key`), as an element that shows nothing. Put it anywhere in the view.

### focused

```rust
fn focused[M]() -> Attr[M]
```

The element takes the keyboard focus when it appears with this attribute, or the attribute appears on it.

### revealed

```rust
fn revealed[M]() -> Attr[M]
```

The page scrolls as little as needed to show the element when it appears with this attribute, or the attribute appears on it.
