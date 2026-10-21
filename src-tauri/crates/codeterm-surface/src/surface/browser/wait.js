(function waitForPage(kind, value, deadlineMs) {
  return new Promise((resolve, reject) => {
    let observer;
    let timer;
    let finished = false;
    const finish = (error) => {
      if (finished) return;
      finished = true;
      observer?.disconnect();
      if (timer !== undefined) clearTimeout(timer);
      if (error) reject(error);
      else resolve(true);
    };
    const check = () => {
      try {
        const found = kind === "text"
          ? Boolean(document.body?.innerText.includes(value))
          : Boolean(document.querySelector(value));
        if (found) finish();
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
