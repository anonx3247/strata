// The client side of a Strata app (`strata serve`). The server renders the
// view; this sends back what the user does, one message at a time, and
// patches the page to match the view the server answers with.
//
// Messages aren't in the page: each handler has a number, which is sent
// back, and the server finds the message in the page it served
// (`std.web.dispatch`).
// - `data-handle-click="2"`: handler 2's message, when the element is clicked.
// - `data-handle-input="3"`: handler 3 makes the message from what the user
//   entered: the field's value, or a form's fields.
// - `data-every="1500" data-handle-every="4"`: sent every 1.5s while the
//   element is on the page (`std.web.every`), skipped while another message
//   is on its way.
// - `data-key="meta+p" data-handle-key="5"`: sent when the key is pressed
//   (`std.web.on_key`).
// - `data-focus`, `data-reveal`: the element takes the focus, or is scrolled
//   into view, when the attribute appears.
//
// The server answers 204 when the message didn't change the model: the page
// stays as it is.
//
// The page is patched, not replaced, so focus and scroll stay, and the field
// being typed in keeps its cursor and what was typed since (after a submit,
// its value is the view's: a form can clear it).
(() => {
  const root = document.getElementById("strata-app");
  if (!root) return;
  let queue = Promise.resolve();

  function banner(text) {
    let b = document.getElementById("strata-error");
    if (!b) {
      b = document.createElement("pre");
      b.id = "strata-error";
      b.style.cssText = "position:fixed;left:0;right:0;bottom:0;margin:0;padding:.75rem 1rem;background:#ffebe9;color:#82071e;border-top:1px solid #ff8182;font:13px ui-monospace,monospace;white-space:pre-wrap;z-index:2147483647";
      document.body.appendChild(b);
    }
    b.textContent = text;
  }

  // ---- patching the page ----

  /// The field whose typing is being sent: the view's value doesn't replace
  /// what it holds.
  let typing = null;

  function morphAttributes(old, neu) {
    for (const a of [...old.attributes]) if (!neu.hasAttribute(a.name)) old.removeAttribute(a.name);
    for (const a of neu.attributes) if (old.getAttribute(a.name) !== a.value) old.setAttribute(a.name, a.value);
    // what a field shows is its property, not its attribute; the field being
    // typed in keeps what the user typed
    if (old === typing) return;
    if (old instanceof HTMLInputElement) {
      if (old.type === "checkbox" || old.type === "radio") old.checked = neu.hasAttribute("checked");
      else old.value = neu.getAttribute("value") ?? "";
    } else if (old instanceof HTMLTextAreaElement) {
      old.value = neu.textContent;
    }
  }

  function morph(old, neu) {
    morphAttributes(old, neu);
    const a = [...old.childNodes];
    const b = [...neu.childNodes];
    b.forEach((n, i) => {
      const o = a[i];
      if (!o) old.appendChild(n);
      else if (o.nodeType === Node.TEXT_NODE && n.nodeType === Node.TEXT_NODE) {
        if (o.nodeValue !== n.nodeValue) o.nodeValue = n.nodeValue;
      } else if (o.nodeType === Node.ELEMENT_NODE && n.nodeType === Node.ELEMENT_NODE && o.tagName === n.tagName) {
        morph(o, n);
      } else old.replaceChild(n, o);
    });
    for (let i = b.length; i < a.length; i++) a[i].remove();
    if (old instanceof HTMLSelectElement && old !== typing) {
      const selected = [...old.options].findIndex((o) => o.hasAttribute("selected"));
      if (selected >= 0) old.selectedIndex = selected;
    }
  }

  function show(html) {
    const before = new Set(root.querySelectorAll("[data-focus], [data-reveal]"));
    const next = document.createElement("div");
    next.innerHTML = html;
    morph(root, Object.assign(next, { id: root.id }));
    // `morph` copied the new root's attributes: keep the session
    root.dataset.session = sessionId;
    arrived(before);
    timers();
  }

  /// Focus, or scroll to, the elements whose `data-focus` or `data-reveal`
  /// wasn't there before (`before`: those that were).
  function arrived(before) {
    for (const el of root.querySelectorAll("[data-focus]")) if (!before.has(el) && el !== document.activeElement) el.focus();
    for (const el of root.querySelectorAll("[data-reveal]")) if (!before.has(el)) el.scrollIntoView({ block: "nearest" });
  }

  // ---- messages sent on a timer (`every`) ----

  /// The running timers, by what they send: "<ms> <handler>".
  const running = new Map();
  /// Messages sent or waiting to be: a timer's message waits for none.
  let busy = 0;

  function timers() {
    const wanted = new Set();
    for (const el of root.querySelectorAll("[data-every]")) {
      const ms = Math.max(100, Number(el.dataset.every) || 0);
      const handler = Number(el.dataset.handleEvery);
      const key = `${ms} ${handler}`;
      wanted.add(key);
      if (!running.has(key)) running.set(key, setInterval(() => { if (!busy) enqueue({ handler }); }, ms));
    }
    for (const [key, t] of running) if (!wanted.has(key)) { clearInterval(t); running.delete(key); }
  }

  // ---- keys (`on_key`) ----

  function keyName(e) {
    if (["Control", "Alt", "Meta", "Shift"].includes(e.key)) return null;
    // with ctrl or meta held, a letter is named in lowercase whatever shift does
    const key = e.key.length === 1 && (e.ctrlKey || e.metaKey) ? e.key.toLowerCase() : e.key;
    return (e.ctrlKey ? "ctrl+" : "") + (e.altKey ? "alt+" : "") + (e.metaKey ? "meta+" : "") + key;
  }

  document.addEventListener("keydown", (e) => {
    const name = keyName(e);
    if (!name) return;
    const t = e.target;
    const typing = t instanceof HTMLElement && (t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName));
    if (typing && !(e.ctrlKey || e.metaKey || name === "Escape")) return;
    const el = [...root.querySelectorAll("[data-key]")].find((k) => k.dataset.key === name);
    if (!el) return;
    e.preventDefault();
    enqueue({ handler: Number(el.dataset.handleKey) });
  }, true);

  const sessionId = root.dataset.session;

  async function send(body, from) {
    try {
      const res = await fetch("/_strata/msg", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ session: sessionId, ...body }),
      });
      const text = await res.text();
      if (res.status === 410) return location.reload();
      if (!res.ok) return banner(`${res.status}: ${text}`);
      document.getElementById("strata-error")?.remove();
      // the model didn't change: neither does the page
      if (res.status === 204) return;
      typing = from;
      show(text);
      typing = null;
    } finally {
      busy--;
    }
  }

  /// `from`: the field whose typing this is, if it is.
  function enqueue(body, from = null) {
    busy++;
    queue = queue.then(() => send(body, from)).catch((err) => banner(String(err)));
  }

  /// What a field holds, as text: a checkbox's is "true" or "false".
  function valueOf(el) {
    if (el instanceof HTMLInputElement && (el.type === "checkbox" || el.type === "radio")) return String(el.checked);
    return "value" in el ? String(el.value) : "";
  }

  function fieldsOf(form) {
    const fields = {};
    for (const [k, v] of new FormData(form)) if (typeof v === "string") fields[k] = v;
    return fields;
  }

  for (const event of ["click", "dblclick", "submit", "change", "input"]) {
    root.addEventListener(event, (e) => {
      const el = e.target.closest(`[data-handle-${event}]`);
      if (!el || !root.contains(el)) return;
      if (event === "click" || event === "dblclick" || event === "submit") e.preventDefault();
      const handler = Number(el.getAttribute(`data-handle-${event}`));
      if (event === "submit") enqueue({ handler, fields: fieldsOf(el) });
      else enqueue({ handler, value: valueOf(el) }, event === "input" ? el : null);
    });
  }

  arrived(new Set());
  timers();
})();
