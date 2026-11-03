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
          const text = node.innerText || node.textContent || "";
          const at = Math.max(0, text.indexOf(value) - 80);
          finish(null, {url:location.href, tag:node.localName, id:node.id, role:node.getAttribute("role"), name:node.getAttribute("aria-label"), excerpt:text.slice(at, at + 240)});
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
