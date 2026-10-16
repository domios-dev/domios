export function frontendPrototypeRoute(search: string): "mission-control" | "gallery" | "snake" | null {
  const route = new URLSearchParams(search).get("prototype");
  return route === "mission-control" || route === "gallery" || route === "snake" ? route : null;
}
