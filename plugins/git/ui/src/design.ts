// Local copy of the app's shared UI class names, scoped to the plugin iframe.
// The corresponding CSS lives in app.css (the iframe has no global app styles).
export const UI_BUTTON = {
  base: "ct-btn",
  primary: "ct-btn ct-btn-primary",
  ghost: "ct-btn ct-btn-ghost",
  accent: "ct-btn ct-btn-accent",
  danger: "ct-btn ct-btn-danger",
  icon: "ct-btn ct-btn-icon ct-btn-ghost",
  sm: "ct-btn ct-btn-sm",
  lg: "ct-btn ct-btn-lg",
} as const;

export const UI_TEXT = {
  meta: "ct-meta",
  metaMuted: "ct-meta-muted",
} as const;

export const UI_PANEL = {
  toolbar: "ct-toolbar",
  toolbarTitle: "ct-toolbar-title",
} as const;
