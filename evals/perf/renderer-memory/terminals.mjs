// In-page helper source: map each mounted `.xterm` element to its xterm Terminal by
// walking React hook state, because the built dev bundle has no debug handle.
export const FIND_TERMINALS = `
function findTerminal(el) {
  const host = el.parentElement;
  const key = host && Object.keys(host).find((name) => name.startsWith("__reactFiber$"));
  for (let fiber = key ? host[key] : null; fiber; fiber = fiber.return) {
    for (let hook = fiber.memoizedState; hook && typeof hook === "object" && "next" in hook; hook = hook.next) {
      const value = hook.memoizedState && hook.memoizedState.current;
      if (value && value.buffer && value.buffer.normal && typeof value.onLineFeed === "function") return value;
    }
  }
  return null;
}
function terminalsIn(root) {
  return [...root.querySelectorAll(".xterm")].map(findTerminal).filter(Boolean);
}
`;
