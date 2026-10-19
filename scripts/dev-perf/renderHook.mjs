// Page-side React commit hook, installed before React loads (DevTools-style global hook).
// It counts components that really rendered in each commit and, for every render that its
// parent did not cause, records why: a changed context value or the index of a changed hook.
export const RENDER_HOOK_SOURCE = String.raw`(() => {
  const stats = { commits: 0, byName: {}, byTrigger: {}, since: performance.now() };
  window.__devPerfRenders = stats;
  const MEMO = 14, FORWARD_REF = 11, HOST_ROOT = 3, PERFORMED_WORK = 1;
  const nameOf = (fiber) => {
    if (fiber.tag === MEMO || fiber.tag === FORWARD_REF) return null;
    const type = fiber.type;
    if (!type || typeof type === "string") return null;
    return type.displayName || type.name || null;
  };
  const reasonsOf = (fiber) => {
    const previous = fiber.alternate;
    if (!previous) return ["mount"];
    const reasons = [];
    let current = fiber.dependencies && fiber.dependencies.firstContext;
    let before = previous.dependencies && previous.dependencies.firstContext;
    while (current) {
      if (!before || current.memoizedValue !== before.memoizedValue) reasons.push("context:" + (current.context.displayName || "anonymous"));
      current = current.next;
      before = before && before.next;
    }
    let hook = fiber.memoizedState, previousHook = previous.memoizedState, index = 0;
    while (hook && typeof hook === "object" && "next" in hook && index < 64) {
      if (hook.queue && previousHook && hook.memoizedState !== previousHook.memoizedState) reasons.push("hook#" + index);
      hook = hook.next;
      previousHook = previousHook && previousHook.next;
      index++;
    }
    return reasons.length ? reasons : ["unknown"];
  };
  const walk = (root) => {
    const stack = [[root, false, "root"]];
    while (stack.length) {
      const [fiber, parentRendered, parentName] = stack.pop();
      if (!fiber) continue;
      const name = nameOf(fiber);
      const rendered = fiber.tag !== HOST_ROOT && !!name && (fiber.flags & PERFORMED_WORK) !== 0;
      if (rendered) {
        stats.byName[name] = (stats.byName[name] || 0) + 1;
        if (!parentRendered) {
          const key = name + " < " + parentName + " [" + reasonsOf(fiber).join(",") + "]";
          stats.byTrigger[key] = (stats.byTrigger[key] || 0) + 1;
        }
      }
      if (fiber.sibling) stack.push([fiber.sibling, parentRendered, parentName]);
      // A skipped subtree keeps the previous commit's child fibers and their stale flags.
      if (fiber.child && (!fiber.alternate || fiber.child !== fiber.alternate.child)) {
        stack.push([fiber.child, name ? rendered : parentRendered, name || parentName]);
      }
    }
  };
  window.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {
    renderers: new Map(),
    supportsFiber: true,
    checkDCE() {},
    inject(renderer) { const id = this.renderers.size + 1; this.renderers.set(id, renderer); return id; },
    onScheduleFiberRoot() {},
    onCommitFiberUnmount() {},
    onPostCommitFiberRoot() {},
    onCommitFiberRoot(_id, root) { stats.commits++; walk(root.current); },
  };
})();`;

export const RESET_RENDERS = `(() => { const s = window.__devPerfRenders; if (!s) return false; s.commits = 0; s.byName = {}; s.byTrigger = {}; s.since = performance.now(); return true; })()`;

export const READ_RENDERS = `(() => {
  const s = window.__devPerfRenders;
  if (!s) return null;
  const top = (map, n) => Object.entries(map).sort((a, b) => b[1] - a[1]).slice(0, n).map(([name, count]) => count + " " + name);
  return { commits: s.commits, seconds: +((performance.now() - s.since) / 1000).toFixed(1), triggers: top(s.byTrigger, 15), rendered: top(s.byName, 15) };
})()`;
