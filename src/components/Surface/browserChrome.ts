import type { SurfaceChromeSpec } from "./surfaceChromeModel";

export const BROWSER_SURFACE_TITLE = "Browser";

export const BROWSER_SURFACE_CHROME: SurfaceChromeSpec = {
  toolbar: [
    { id: "back", kind: "button", icon: "chevron-left", label: "Back", when: "interactive", invoke: { method: "nav", args: { action: "back" } } },
    { id: "forward", kind: "button", icon: "chevron-right", label: "Forward", when: "interactive", invoke: { method: "nav", args: { action: "forward" } } },
    { id: "reload", kind: "button", icon: "rotate-cw", label: "Reload", when: "interactive", invoke: { method: "nav", args: { action: "reload" } } },
    { id: "address", kind: "input", label: "Address", placeholder: "Address", source: "url", when: "interactive", invoke: { method: "nav", args: { action: "goto" }, valueArg: "target" } },
    { id: "profile", kind: "select", label: "Browser profile", source: "profiles", invoke: { method: "profiles.use", valueArg: "name" } },
  ],
  takeControl: true,
  pointAndAsk: true,
  snip: true,
  openExternal: true,
};
