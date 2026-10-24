export function consumeWebEntry(location: Pick<Location, "pathname" | "search" | "hash">) {
  const hash = new URLSearchParams(location.hash.replace(/^#/, ""));
  const query = new URLSearchParams(location.search);
  const hasEntry = hash.has("token") || query.has("token") || hash.has("relay");
  const token = (hash.get("token") ?? query.get("token"))?.trim();
  const relay = hash.get("relay")?.trim();
  query.delete("token");
  const search = query.toString();
  return {
    hasEntry,
    token,
    relay,
    cleanUrl: location.pathname + (search ? `?${search}` : "") + (hasEntry ? "" : location.hash),
  };
}
