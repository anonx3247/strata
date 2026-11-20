// The client side of a Strata app (`strata serve`). The server renders the
// view; this sends back the message an element carries when the user acts
// on it (`data-on-click="<message as JSON>"`), one message at a time, and
// puts the view the server answers with in place.
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

  async function send(msg) {
    const res = await fetch("/_strata/msg", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ session: root.dataset.session, msg: JSON.parse(msg) }),
    });
    const text = await res.text();
    if (res.status === 410) return location.reload();
    if (!res.ok) return banner(`${res.status}: ${text}`);
    document.getElementById("strata-error")?.remove();
    root.innerHTML = text;
  }

  for (const event of ["click", "dblclick", "submit", "change"]) {
    root.addEventListener(event, (e) => {
      const el = e.target.closest(`[data-on-${event}]`);
      if (!el || !root.contains(el)) return;
      e.preventDefault();
      const msg = el.getAttribute(`data-on-${event}`);
      queue = queue.then(() => send(msg)).catch((err) => banner(String(err)));
    });
  }
})();
