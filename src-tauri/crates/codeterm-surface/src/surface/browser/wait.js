(function waitForPage(kind, value, deadlineMs) {
  return new Promise((resolve, reject) => {
    let observer;
    let timer;
    let finished = false;
    const finish = (error, evidence) => {
      if (finished) return;
      finished = true;
      observer?.disconnect();
      if (timer !== undefined) clearTimeout(timer);
      if (error) reject(error);
      else resolve(evidence);
    };
    const check = () => {
      try {
        let node;
        if (kind === "text") {
          node = document.body;
          if (!node?.innerText.includes(value)) return;
          while (true) {
            const child = Array.from(node.children).find(child => child.innerText?.includes(value));
            if (!child) break;
            node = child;
          }
        } else node = document.querySelector(value);
        if (node) {
          let text = node.innerText || node.textContent || "";
          const sensitive = '[data-sensitive], input[type="password"], input[autocomplete="current-password"], input[autocomplete="new-password"]';
          const masked = node.matches(sensitive) || node.closest('[data-sensitive]');
          if (masked) text = "REDACTED";
          else for (const child of node.querySelectorAll(sensitive)) {
            if (child.innerText) text = text.split(child.innerText).join("REDACTED");
          }
          const at = Math.max(0, text.indexOf(value) - 80);
          finish(null, {url:location.href, tag:node.localName, id:node.id.slice(0, 200), role:node.getAttribute("role")?.slice(0, 100), name:masked ? "REDACTED" : node.getAttribute("aria-label")?.slice(0, 200), excerpt:text.slice(at, at + 240)});
        }
      } catch (error) {
        finish(error);
      }
    };
    check();
    if (finished) return;
    observer = new MutationObserver(check);
    observer.observe(document, {
      subtree: true, childList: true, characterData: true, attributes: true,
    });
    timer = setTimeout(() => finish(new Error("page condition deadline exceeded")), deadlineMs);
  });
})
