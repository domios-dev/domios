(() => {
  let generation;
  let acknowledged = false;
  let parsed = document.readyState !== "loading";
  const prepare = () => {
    const style = document.createElement("style");
    style.textContent = "header, .stage > * { animation: none !important; }";
    document.head.append(style);
  };
  const acknowledge = () => {
    if (!parsed || generation === undefined || acknowledged) return;
    const main = document.querySelector("main[role]");
    const mark = main?.querySelector("svg");
    const status = main?.querySelector("h1");
    const markBox = mark?.getBoundingClientRect();
    const statusBox = status?.getBoundingClientRect();
    const ready = status?.textContent?.trim()
      && markBox?.width > 0 && markBox?.height > 0
      && statusBox?.width > 0 && statusBox?.height > 0;
    acknowledged = true;
    const scheme = ready ? "domios-starting-ready" : "domios-starting-unavailable";
    location.href = `${scheme}://localhost/${generation}`;
  };
  if (!parsed) {
    document.addEventListener("DOMContentLoaded", () => {
      parsed = true;
      prepare();
      acknowledge();
    }, { once: true });
  } else {
    prepare();
  }
  window.__DOMIOS_STARTING_READY__ = requestedGeneration => {
    generation = requestedGeneration;
    acknowledge();
  };
})();
