(() => {
  const vis = typeof document.visibilityState === 'string' ? document.visibilityState : 'unknown';
  const body = document.body;
  const text = ((body && body.innerText) || '').trim();
  const hasText = text.length > 0;
  let visibleElements = 0;
  const vw = Number.isFinite(window.innerWidth) ? window.innerWidth : 0;
  const vh = Number.isFinite(window.innerHeight) ? window.innerHeight : 0;
  if (vw > 0 && vh > 0 && body) {
    const nodes = body.querySelectorAll(
      'a,button,input,textarea,select,img,svg,canvas,video,h1,h2,h3,p,span,article,main,[role]'
    );
    for (let i = 0; i < nodes.length && i < 400 && visibleElements < 3; i++) {
      const el = nodes[i];
      const r = el.getBoundingClientRect();
      if (r.width <= 0 || r.height <= 0) continue;
      if (r.bottom <= 0 || r.top >= vh || r.right <= 0 || r.left >= vw) continue;
      const s = getComputedStyle(el);
      if (s.visibility === 'hidden' || s.display === 'none' || parseFloat(s.opacity) === 0) continue;
      visibleElements++;
    }
  }
  return {
    visibilityState: vis,
    hidden: Boolean(document.hidden),
    hasFocus: typeof document.hasFocus === 'function' ? Boolean(document.hasFocus()) : true,
    hasText,
    visibleElements,
    viewportWidth: vw,
    viewportHeight: vh,
  };
})()
