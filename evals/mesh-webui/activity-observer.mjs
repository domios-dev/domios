export function installSelectedActivityObserver({tabId, doc = document, host = window,
  Observer = MutationObserver, now = Date.now, monotonic = () => performance.now()}) {
  if (!tabId) throw new Error('Selected activity tab identity is required');
  host.__b2Activity?.observer.disconnect();
  const events = [];
  let prior = '';
  const collect = () => {
    const roots = [...doc.querySelectorAll('[data-testid="tab-session-chat"]')].filter(root =>
      root.getAttribute('data-tab-id') === tabId && root.checkVisibility({checkVisibilityCSS:true,checkOpacity:true})
      && [...root.getClientRects()].some(rect => rect.width > 0 && rect.height > 0));
    const chat = roots.length === 1 ? roots[0] : null;
    const agent = chat?.querySelector('[data-testid="agent-state"]');
    const log = chat?.querySelector('[role="log"]');
    const row = {tabId, available:!!chat, state:agent?.getAttribute('data-agent-state'),
      phase:agent?.getAttribute('data-agent-phase'), status:agent?.textContent,
      sending:!!chat?.querySelector('[data-state="sending"]'), progress:log?.textContent?.slice(-4000),
      elapsed:agent?.textContent?.match(/\b\d+(?:\.\d+)?\s*(?:s|sec|seconds|m|min)\b/)?.[0] ?? null};
    const serialized = JSON.stringify(row);
    if (serialized !== prior) {events.push({...row,at:now(),monotonic:monotonic()});prior=serialized;}
  };
  const observer = new Observer(collect);
  observer.observe(doc.documentElement,{subtree:true,childList:true,attributes:true,characterData:true});
  host.__b2Activity = {observer,events};
  collect();
}
