// Test harness for film-embed.html: lazy-mounts the built film the way the site will and records isolation evidence.
const MODULE = "/dist-film/index.js";
const MEDIA = "/dist-film/public";
const params = new URLSearchParams(location.search);
const slot = document.getElementById("film-slot");
const log = document.getElementById("probe-log");

let frames = 0;
const nativeRaf = window.requestAnimationFrame.bind(window);
window.requestAnimationFrame = (callback) => nativeRaf((now) => {
  frames++;
  callback(now);
});

const media = new Set();
const nativePlay = HTMLMediaElement.prototype.play;
HTMLMediaElement.prototype.play = function play() {
  media.add(this);
  return nativePlay.call(this);
};

const hostStyle = () => {
  const root = getComputedStyle(document.documentElement);
  const body = getComputedStyle(document.body);
  return {
    "--ct-bg": root.getPropertyValue("--ct-bg").trim(),
    "--ct-fg": root.getPropertyValue("--ct-fg").trim(),
    "--ct-accent": root.getPropertyValue("--ct-accent").trim(),
    "html font-size": root.fontSize,
    "body font-family": body.fontFamily,
    "body background": body.backgroundColor,
    "html data-ct-theme": document.documentElement.dataset.ctTheme ?? null,
    "body children": document.body.children.length,
    "head styles": document.head.querySelectorAll("style, link").length,
  };
};
const baseline = hostStyle();
const requests = () => performance.getEntriesByType("resource").map((e) => e.name.replace(location.origin, "")).filter((n) => n.startsWith("/dist-film"));

const evidence = { baseline, moduleRequestedAt: null, mounted: false, framesAfterDestroy: null };
window.__filmEvidence = evidence;

function report(extra = {}) {
  Object.assign(evidence, extra, {
    hostNow: hostStyle(),
    hostUnchanged: JSON.stringify(hostStyle()) === JSON.stringify(baseline),
    mediaRequests: requests().filter((n) => /\.(m4a|mp4|json)$/.test(n)),
    moduleRequests: requests().filter((n) => n.endsWith(".js")).length,
    filmNodesOutsideHost: document.querySelectorAll("body > :not(section, aside, script)").length,
    storageKeys: localStorage.length + sessionStorage.length,
    mediaPlaying: [...media].filter((m) => !m.paused).length,
  });
  log.textContent = JSON.stringify(evidence, null, 1);
}

let film = null;
let mountFilm = null;
async function mount() {
  if (film) return;
  if (!mountFilm) {
    evidence.moduleRequestedAt = Math.round(performance.now());
    ({ mountFilm } = await import(MODULE));
  }
  const reduced = params.has("reduced") ? params.get("reduced") === "1" : matchMedia("(prefers-reduced-motion: reduce)").matches;
  film = mountFilm(slot, {
    reel: "web-hale",
    audioUrl: `${MEDIA}/web-hale.m4a`,
    captionsUrl: `${MEDIA}/web-hale.captions.json`,
    theme: params.get("theme") === "light" ? "light" : "dark",
    layout: params.get("layout") === "portrait" ? "portrait" : "landscape",
    reducedMotion: reduced,
  });
  window.__film = film;
  report({ mounted: true, framesAfterDestroy: null });
}

const observer = new IntersectionObserver((entries) => {
  if (!entries.some((e) => e.isIntersecting)) return;
  observer.disconnect();
  void mount();
}, { rootMargin: "200px 0px" });
observer.observe(slot);

const actions = {
  dark: () => film?.setTheme("dark"),
  light: () => film?.setTheme("light"),
  landscape: () => film?.setLayout("landscape"),
  portrait: () => film?.setLayout("portrait"),
  pause: () => film?.pause(),
  mount: () => void mount(),
  destroy: () => {
    film?.destroy();
    film = null;
    window.__film = null;
    const start = frames;
    report({ mounted: false, framesAfterDestroy: "measuring" });
    setTimeout(() => report({ framesAfterDestroy: frames - start, filmHostsLeft: slot.childElementCount }), 1000);
  },
};
document.getElementById("probe").addEventListener("click", (e) => {
  const action = e.target instanceof HTMLElement ? e.target.dataset.action : undefined;
  if (!action) return;
  actions[action]();
  report();
});
setInterval(report, 1000);
report();
