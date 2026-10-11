export function sidebarDrawerLayout(phone: boolean) {
  if (!phone) return {};

  // The shell body already starts below the titlebar, including its safe-area padding.
  return {
    position: "absolute" as const,
    top: 0,
    bottom: 0,
    left: 0,
    zIndex: 40,
    paddingBottom: "var(--ct-safe-bottom, 0px)",
  };
}
