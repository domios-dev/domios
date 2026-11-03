(() => {
  const key = Symbol.for('domios.browser.presentation');
  if (window[key]) return;
  const host = document.createElement('domios-agent-cursor');
  host.setAttribute('aria-hidden', 'true');
  host.style.cssText = 'all:initial!important;color-scheme:light dark!important;position:fixed!important;inset:0!important;width:0!important;height:0!important;z-index:2147483647!important;pointer-events:none!important;';
  const root = host.attachShadow({mode: 'closed'});
  const sheet = new CSSStyleSheet();
  sheet.replaceSync(`
    :host { --blue:#2679ff; --edge:#fff; --on-blue:#fff; color-scheme:light dark; }
    * { box-sizing:border-box; pointer-events:none!important; }
    @media(prefers-color-scheme:dark) { :host { --blue:#62a4ff; --edge:#0d1117; --on-blue:#0d1117; } }
    .cursor { position:fixed;left:0;top:0;width:0;height:0;filter:drop-shadow(0 1px 2px #0d1b3a40); }
    svg { position:absolute;left:0;top:0;width:12px;height:15px;overflow:visible; }
    .tag { position:absolute;left:9px;top:13px;display:flex;align-items:center;gap:4px;max-width:140px;height:16px;padding:0 7px 0 5px;border-radius:8px;background:var(--blue);color:var(--on-blue);box-shadow:0 0 0 1.5px var(--edge);font:600 10.5px/16px system-ui,sans-serif;white-space:nowrap;transform-origin:0 0; }
    .tag::before { content:'';flex:none;width:5px;height:5px;border-radius:50%;background:currentColor;opacity:.85; }
    .tag span { min-width:0;overflow:hidden;text-overflow:ellipsis; }
    @keyframes tagin { from { opacity:0;transform:scale(.6); } to { opacity:1;transform:scale(1); } }
    .ripple { position:absolute;left:-13px;top:-13px;width:26px;height:26px;border:2px solid var(--blue);border-radius:50%;opacity:0; }
    .trail { position:fixed;height:3px;transform-origin:left;border-radius:9px;background:linear-gradient(90deg,transparent,var(--blue));opacity:.4; }
    .field { position:fixed;border:2px solid var(--blue);border-radius:7px;box-shadow:0 0 0 4px #388cff22; }
    .typing { position:absolute;left:0;top:100%;margin-top:8px;padding:5px 9px;border-radius:7px;background:light-dark(#edf5ff,#122b49);color:light-dark(#145cc6,#b8d8ff);font:12px/18px system-ui;white-space:pre;max-width:280px;overflow:hidden;box-shadow:0 2px 12px #12396e22; }
    @keyframes travel { from { transform:translate(var(--from-x),var(--from-y)); } to { transform:translate(var(--to-x),var(--to-y)); } }
    @keyframes press { from { opacity:.8;transform:scale(.4); } to { opacity:0;transform:scale(1.8); } }
    @keyframes reveal { from { clip-path:inset(0 100% 0 0); } to { clip-path:inset(0); } }
    @keyframes caret { 0%,100% { opacity:.65; } 50% { opacity:1; } }
  `);
  root.adoptedStyleSheets = [sheet];
  function layer(name, parent = root) {
    const node = document.createElement('div');
    node.className = name;
    parent.append(node);
    return node;
  }
  const cursorLayer = layer('cursor');
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  // The tip is the SVG origin, so translate(x,y) puts the hotspot exactly on the target.
  svg.setAttribute('viewBox', '0 0 12 15');
  const arrow = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  for (const [name,value] of Object.entries({d:'M0 0 L11.5 8.6 L6.6 9.6 L4.1 14.3 Z',fill:'var(--blue)',stroke:'var(--edge)','stroke-width':'1.6','stroke-linejoin':'round','paint-order':'stroke'})) arrow.setAttribute(name,value);
  svg.append(arrow);
  cursorLayer.append(svg);
  const tag = layer('tag',cursorLayer);
  const tagName = document.createElement('span');
  tag.append(tagName);
  tag.hidden = true;
  layer('ripple',cursorLayer);
  layer('trail');
  layer('typing',layer('field'));
  const cursor = root.querySelector('.cursor');
  const ripple = root.querySelector('.ripple');
  const trail = root.querySelector('.trail');
  const field = root.querySelector('.field');
  const typing = root.querySelector('.typing');
  cursor.hidden = field.hidden = trail.hidden = true;
  const reduced = window.matchMedia('(prefers-reduced-motion: reduce)');
  let point = {x:24,y:24};
  let finish = null;
  let suppressed = 0;
  let generation = 0;
  function mount() {
    if (!host.isConnected && document.documentElement) document.documentElement.append(host);
  }
  const observer = new MutationObserver(mount);
  observer.observe(document, {childList:true,subtree:true});
  mount();
  function hide() {
    generation++;
    if (finish) finish();
    cursor.hidden = field.hidden = trail.hidden = true;
  }
  function animate(node, name, duration, easing = 'ease') {
    if (reduced.matches) return Promise.resolve();
    return new Promise(resolve => {
      const end = event => { if (event.target === node) done(); };
      const done = () => {
        node.removeEventListener('animationend', end);
        node.removeEventListener('animationcancel', end);
        node.style.animation = '';
        if (finish === done) finish = null;
        resolve();
      };
      finish = done;
      node.addEventListener('animationend', end);
      node.addEventListener('animationcancel', end);
      node.style.animation = `${name} ${duration}ms ${easing} both`;
    });
  }
  async function present(request, target) {
    if (finish) finish();
    const current = ++generation;
    mount();
    if (suppressed || !host.isConnected) return;
    field.hidden = trail.hidden = true;
    const rect = target?.getBoundingClientRect();
    const x = request.x ?? (rect ? rect.x + rect.width / 2 : point.x);
    const y = request.y ?? (rect ? rect.y + rect.height / 2 : point.y);
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    const name = typeof request.name === 'string' ? request.name.trim() : '';
    const appearing = cursor.hidden || tagName.textContent !== name;
    tagName.textContent = name;
    tag.hidden = !name;
    cursor.hidden = false;
    if (name && appearing && !reduced.matches) {
      tag.style.animation = 'none';
      void tag.offsetWidth;
      tag.style.animation = 'tagin 160ms cubic-bezier(.2,.8,.2,1) both';
    }
    cursor.style.setProperty('--from-x', `${point.x}px`);
    cursor.style.setProperty('--from-y', `${point.y}px`);
    cursor.style.setProperty('--to-x', `${x}px`);
    cursor.style.setProperty('--to-y', `${y}px`);
    cursor.style.transform = `translate(${x}px,${y}px)`;
    if (request.kind === 'drag' && !reduced.matches) {
      trail.hidden = false;
      trail.style.cssText = `left:${point.x}px;top:${point.y}px;width:${Math.hypot(x-point.x,y-point.y)}px;transform:rotate(${Math.atan2(y-point.y,x-point.x)}rad)`;
    }
    point = {x,y};
    await animate(cursor, 'travel', request.kind === 'drag' ? 90 : 140, 'cubic-bezier(.2,.8,.2,1)');
    if (current !== generation) return;
    trail.hidden = true;
    if (request.kind === 'click') await animate(ripple, 'press', 100);
    if (current !== generation) return;
    if (request.kind === 'typing' && rect) {
      field.hidden = false;
      field.style.cssText = `left:${rect.x-3}px;top:${rect.y-3}px;width:${rect.width+6}px;height:${rect.height+6}px`;
      const masked = request.secret || target?.matches('input[type="password"], [data-sensitive], [data-sensitive] *');
      const text = masked ? '' : request.text || '';
      typing.textContent = masked ? '••••  ▏' : text.length > 48 ? 'Typing  ▏' : text + '▏';
      await animate(typing, masked || text.length > 48 ? 'caret' : 'reveal', 160, masked || text.length > 48 ? 'ease' : `steps(${Math.max(1,text.length)})`);
    }
  }
  window.addEventListener('pagehide', hide);
  reduced.addEventListener('change', () => { if (finish) finish(); });
  Object.defineProperty(window, key, {value:Object.freeze({
    present,
    hide,
    capture(hideOverlay) {
      suppressed = Math.max(0, suppressed + (hideOverlay ? 1 : -1));
      host.style.setProperty('visibility', suppressed ? 'hidden' : 'visible', 'important');
    }
  })});
})()
