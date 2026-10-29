(() => {
  let requested = false;
  const prepare = () => {
    const style = document.createElement("style");
    style.textContent = "header, .stage > * { animation: none !important; }";
    document.head.append(style);
  };
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", prepare, { once: true });
  } else {
    prepare();
  }
  window.__DOMIOS_STARTING_PAINT__ = generation => {
    const main = document.querySelector("main[role]");
    const mark = main?.querySelector("svg");
    const status = main?.querySelector("h1");
    if (requested || !mark || !status?.textContent?.trim()) return;
    requested = true;
    requestAnimationFrame(() => {
      if (mark.getBoundingClientRect().width === 0 || status.getBoundingClientRect().height === 0) return;
      requestAnimationFrame(() => {
        location.href = `domios-starting-painted://localhost/${generation}`;
      });
    });
  };
})();
