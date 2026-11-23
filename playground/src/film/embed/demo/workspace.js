// Test harness for workspace-embed.html: mounts the built workspace the way the site will, with chips for every place.
// ?theme=light|dark&layout=landscape|portrait&place=<id>&reduced=1&pauseAt=<seconds>
const params = new URLSearchParams(location.search);
const PLACES = ["chat", "machines", "plan", "teams", "plugins", "usage"];
const state = {
  theme: params.get("theme") === "light" ? "light" : "dark",
  layout: params.get("layout") === "portrait" ? "portrait" : "landscape",
  place: PLACES.includes(params.get("place")) ? params.get("place") : "chat",
};
const controls = document.getElementById("controls");
const log = document.getElementById("log");
const note = (line) => { log.textContent = `${line}\n${log.textContent}`.slice(0, 4000); };
window.addEventListener("error", (event) => note(`error ${event.message}`));
window.addEventListener("unhandledrejection", (event) => note(`rejection ${event.reason?.stack ?? event.reason}`));
const nativeError = console.error.bind(console);
console.error = (...args) => {
  note(`console.error ${args.map((a) => (a instanceof Error ? a.stack : String(a))).join(" ").slice(0, 600)}`);
  nativeError(...args);
};
for (const name of ["codeterm-peer-switch-reset", "codeterm-peer-switch-rebind", "codeterm-peer-switch-ready"]) {
  window.addEventListener(name, (event) => note(`${name} ${JSON.stringify(event.detail)}`));
}
document.body.dataset.theme = state.theme;
document.body.dataset.layout = state.layout;

const { mountWorkspace } = await import("/dist-film/index.js");
const handle = mountWorkspace(document.getElementById("slot"), {
  ...state,
  reducedMotion: params.get("reduced") === "1",
  onPlace: (place) => {
    state.place = place;
    note(`onPlace ${place}`);
    paint();
  },
});
window.__workspace = handle;
// ?pauseAt=SECONDS freezes the story through the host's own pause, for a still of one moment.
if (params.get("pauseAt")) setTimeout(() => handle.pause(), Number(params.get("pauseAt")) * 1000);

function button(label, current, onClick) {
  const el = document.createElement("button");
  el.type = "button";
  el.textContent = label;
  if (current) el.setAttribute("aria-current", "true");
  el.addEventListener("click", onClick);
  return el;
}

function paint() {
  controls.replaceChildren(
    ...PLACES.map((place) => button(place, state.place === place, () => {
      state.place = place;
      handle.setPlace(place, { replay: true });
      paint();
    })),
    button(state.theme, false, () => {
      state.theme = state.theme === "dark" ? "light" : "dark";
      document.body.dataset.theme = state.theme;
      handle.setTheme(state.theme);
      paint();
    }),
    button(state.layout, false, () => {
      state.layout = state.layout === "landscape" ? "portrait" : "landscape";
      document.body.dataset.layout = state.layout;
      handle.setLayout(state.layout);
      paint();
    }),
    button("composer rect", false, () => note(`composer ${JSON.stringify(handle.anchor("composer"))}`)),
  );
}
paint();
