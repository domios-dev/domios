export function ownsViewport(viewer: string, owner: string | undefined): boolean {
  return viewer === owner;
}

export function browserViewerTarget(context: object = globalThis): string | undefined {
  const value: unknown = Reflect.get(context, Symbol.for("domios.browser.target"));
  return typeof value === "string" ? value : undefined;
}
