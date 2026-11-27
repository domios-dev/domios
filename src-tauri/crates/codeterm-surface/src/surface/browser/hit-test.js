function domiosHitTest(x, y) {
  const targetEl = this;
  if (!targetEl || !targetEl.isConnected) return 'stale: detached pointer target';
  const doc = targetEl.ownerDocument;
  if (!doc || !doc.defaultView || doc.defaultView !== doc.defaultView.top) {
    return 'target_occluded: cannot verify an embedded frame target';
  }
  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    return 'target_occluded: invalid pointer coordinates';
  }

  const targetRoots = new Map();
  for (let node = targetEl; node && node.getRootNode; node = node.getRootNode().host) {
    const root = node.getRootNode();
    if (!root || !root.host) break;
    targetRoots.set(root.host, root);
  }

  const stack = [];
  const seen = new Set();
  const visitedRoots = new Set();
  function pierce(root) {
    if (!root || visitedRoots.has(root)) return;
    visitedRoots.add(root);
    let hits = [];
    if (typeof root.elementsFromPoint === 'function') {
      hits = Array.from(root.elementsFromPoint(x, y));
    } else if (typeof root.elementFromPoint === 'function') {
      const top = root.elementFromPoint(x, y);
      if (top) hits = [top];
    }
    for (const el of hits) {
      if (!el || seen.has(el)) continue;
      const inner = targetRoots.get(el) || el.shadowRoot;
      if (inner && !visitedRoots.has(inner)) pierce(inner);
      if (!seen.has(el)) {
        seen.add(el);
        stack.push(el);
        if (stack.length >= 24) return;
      }
    }
  }
  pierce(doc);
  if (!stack.length) return 'target_occluded: no element at pointer coordinates';

  const win = doc.defaultView;
  const styleOf = (el) => {
    try {
      return win.getComputedStyle(el);
    } catch {
      return null;
    }
  };

  function colorAlpha(color) {
    if (!color || color === 'transparent') return 0;
    const rgba = /^rgba?\(([^)]+)\)$/.exec(color);
    if (rgba) {
      const parts = rgba[1].split(/[,\s/]+/).filter(Boolean);
      if (parts.length >= 4) {
        const raw = parts[3];
        const a = raw.endsWith('%') ? parseFloat(raw) / 100 : parseFloat(raw);
        return Number.isFinite(a) ? a : 1;
      }
      return 1;
    }
    const slash = /\/\s*([0-9.]+%?)\s*\)$/.exec(color);
    if (slash) {
      const raw = slash[1];
      const a = raw.endsWith('%') ? parseFloat(raw) / 100 : parseFloat(raw);
      return Number.isFinite(a) ? a : 1;
    }
    return 1;
  }

  const SENSITIVE = '[data-sensitive], input[type="password"]';
  const INTERACTIVE_ROLES = new Set([
    'link', 'button', 'checkbox', 'radio', 'switch', 'menuitem',
    'menuitemcheckbox', 'menuitemradio', 'option', 'tab', 'textbox',
    'searchbox', 'combobox', 'listbox', 'slider', 'spinbutton', 'treeitem',
  ]);
  const CLICKABLE_CONTAINER_ROLES = new Set([
    'link', 'button', 'menuitem', 'option', 'tab', 'treeitem',
  ]);
  const LANDMARK_TAGS = new Set([
    'html', 'body', 'main', 'nav', 'header', 'footer', 'aside', 'form',
    'dialog', 'ul', 'ol', 'menu',
  ]);
  const LANDMARK_ROLES = new Set([
    'main', 'navigation', 'banner', 'contentinfo', 'complementary', 'region',
    'feed', 'list', 'form', 'search', 'dialog', 'alertdialog', 'menu',
    'menubar', 'listbox', 'tooltip',
  ]);
  const MEDIA_TAGS = new Set(['img', 'svg', 'canvas', 'video', 'iframe', 'object', 'embed']);
  const INTERACTIVE_SELECTOR = [
    'a[href]', 'button', 'input:not([type="hidden"])', 'select', 'textarea',
    'summary', 'iframe', '[contenteditable=""]', '[contenteditable="true"]',
    '[role="button"]', '[role="link"]', '[role="menuitem"]', '[role="tab"]',
    '[role="checkbox"]', '[role="radio"]', '[role="switch"]', '[role="textbox"]',
    '[role="combobox"]', '[role="dialog"]', '[role="alertdialog"]',
    'dialog[open]', '[aria-modal="true"]',
  ].join(', ');
  const MODAL_SELECTOR = 'dialog[open], [role="dialog"], [role="alertdialog"], [aria-modal="true"]';

  const bodyStyle = doc.body ? styleOf(doc.body) : null;
  const docStyle = doc.documentElement ? styleOf(doc.documentElement) : null;
  const rootCursorPointer =
    (bodyStyle && bodyStyle.cursor === 'pointer') ||
    (docStyle && docStyle.cursor === 'pointer');

  function cleanText(raw) {
    return String(raw || '').replace(/\s+/g, ' ').trim().slice(0, 80);
  }

  function isSensitive(el) {
    return Boolean(el && el.closest && el.closest(SENSITIVE));
  }

  function roleOf(el) {
    const raw = (el.getAttribute && el.getAttribute('role')) || '';
    const explicit = raw.trim().split(/\s+/)[0] || '';
    const lower = explicit.toLowerCase();
    if (lower && lower !== 'presentation' && lower !== 'none') return lower;
    const tag = (el.localName || '').toLowerCase();
    switch (tag) {
      case 'dialog': return 'dialog';
      case 'a': return el.hasAttribute && el.hasAttribute('href') ? 'link' : '';
      case 'button':
      case 'summary': return 'button';
      case 'textarea': return 'textbox';
      case 'select': return el.multiple || (el.size && el.size > 1) ? 'listbox' : 'combobox';
      case 'input': {
        const kind = String(el.type || 'text').toLowerCase();
        if (kind === 'checkbox') return 'checkbox';
        if (kind === 'radio') return 'radio';
        if (['button', 'submit', 'reset', 'image'].includes(kind)) return 'button';
        if (kind === 'search') return 'searchbox';
        if (kind === 'range') return 'slider';
        if (kind === 'number') return 'spinbutton';
        return 'textbox';
      }
      case 'nav': return 'navigation';
      case 'header': return 'banner';
      case 'footer': return 'contentinfo';
      case 'aside': return 'complementary';
      case 'main': return 'main';
      case 'form': return 'form';
      case 'ul':
      case 'ol':
      case 'menu': return 'list';
      case 'li': return 'listitem';
      case 'article': return 'article';
      case 'img': return 'img';
      case 'iframe': return 'iframe';
      case 'h1':
      case 'h2':
      case 'h3':
      case 'h4':
      case 'h5':
      case 'h6': return 'heading';
      default: return '';
    }
  }

  function safeText(el) {
    if (!el || isSensitive(el)) return '';
    if (el.querySelector && el.querySelector(SENSITIVE)) return '';
    return cleanText(el.textContent);
  }

  function nameOf(el, role) {
    if (!el || isSensitive(el)) return '';
    const ariaLabel = cleanText(el.getAttribute && el.getAttribute('aria-label'));
    if (ariaLabel) return ariaLabel;
    const labelledBy = cleanText(el.getAttribute && el.getAttribute('aria-labelledby'));
    if (labelledBy) {
      const root = el.getRootNode && el.getRootNode();
      const parts = [];
      for (const id of labelledBy.split(' ')) {
        const ref = (root && root.getElementById && root.getElementById(id)) || doc.getElementById(id);
        const text = safeText(ref);
        if (text) parts.push(text);
      }
      const joined = cleanText(parts.join(' '));
      if (joined) return joined;
    }
    const tag = (el.localName || '').toLowerCase();
    if (tag === 'img' || tag === 'area' || tag === 'input') {
      const alt = cleanText(el.getAttribute && el.getAttribute('alt'));
      if (alt) return alt;
    }
    const title = cleanText(el.getAttribute && el.getAttribute('title'));
    if (title) return title;
    if (role === 'dialog' || role === 'alertdialog' || tag === 'dialog') {
      const heading = el.querySelector && el.querySelector('h1, h2, h3, [role="heading"]');
      const text = safeText(heading);
      if (text) return text;
    }
    if (tag === 'input' && ['button', 'submit', 'reset'].includes(String(el.type || '').toLowerCase())) {
      const value = cleanText(el.value);
      if (value) return value;
    }
    if (INTERACTIVE_ROLES.has(role) || role === 'heading' || tag === 'label') {
      const text = safeText(el);
      if (text) return text;
    }
    return '';
  }

  function hasVisibleText(el) {
    if (!el || !el.textContent || !el.textContent.trim()) return false;
    if (typeof doc.createTreeWalker !== 'function') return true;
    const walker = doc.createTreeWalker(el, 4);
    let node;
    while ((node = walker.nextNode())) {
      if (!node.nodeValue || !node.nodeValue.trim()) continue;
      const parent = node.parentElement;
      if (!parent) return true;
      const ps = styleOf(parent);
      if (!ps) return true;
      if (ps.visibility === 'hidden' || ps.display === 'none') continue;
      if (parseFloat(ps.opacity) === 0 || colorAlpha(ps.color) === 0 || parseFloat(ps.fontSize) === 0) continue;
      const rect = parent.getBoundingClientRect && parent.getBoundingClientRect();
      if (rect && rect.width <= 2 && rect.height <= 2) continue;
      return true;
    }
    return false;
  }

  function isTransparent(el, style) {
    if (!style) return false;
    const opacity = parseFloat(style.opacity);
    if (Number.isFinite(opacity) && opacity === 0) return true;
    if (colorAlpha(style.backgroundColor) > 0) return false;
    if (style.backgroundImage && style.backgroundImage !== 'none' && style.backgroundImage !== 'initial') return false;
    const tag = (el.localName || '').toLowerCase();
    if (MEDIA_TAGS.has(tag)) return false;
    if (el.querySelector && el.querySelector('img, svg, canvas, video, iframe, object, embed')) {
      return false;
    }
    return !hasVisibleText(el);
  }

  function isInteractive(el, tag, role) {
    if (INTERACTIVE_ROLES.has(role)) return true;
    if (tag === 'a' && el.hasAttribute && el.hasAttribute('href')) return true;
    if (tag === 'input' && String(el.type || '').toLowerCase() !== 'hidden') return true;
    if (['button', 'select', 'textarea', 'summary', 'option', 'iframe'].includes(tag)) return true;
    if (el.isContentEditable) return true;
    const tabIndexAttr = el.getAttribute && el.getAttribute('tabindex');
    if (tabIndexAttr !== null && tabIndexAttr !== '') {
      const parsed = parseInt(tabIndexAttr, 10);
      if (Number.isFinite(parsed) && parsed >= 0) return true;
    }
    return false;
  }

  function parentOf(node) {
    let cur = node;
    while (cur) {
      const next = cur.assignedSlot || cur.parentNode || cur.host || null;
      if (!next || next === doc || next.nodeType === 9) return null;
      if (next.nodeType === 11) {
        cur = next.host || null;
        if (cur && cur.nodeType === 1) return cur;
        continue;
      }
      if (next.nodeType === 1) return next;
      cur = next;
    }
    return null;
  }

  function rectOf(el) {
    if (!el || typeof el.getBoundingClientRect !== 'function') return null;
    const r = el.getBoundingClientRect();
    if (!r || !Number.isFinite(r.left) || !Number.isFinite(r.top)) return null;
    return { left: r.left, top: r.top, width: r.width, height: r.height };
  }

  const associatedLabels = new Set();
  if (targetEl.labels) {
    for (const label of targetEl.labels) associatedLabels.add(label);
  }

  const stackSet = new Set(stack);
  const idByEl = new Map();
  const nodes = [];

  function register(el) {
    if (idByEl.has(el)) return idByEl.get(el);
    const parentEl = parentOf(el);
    const parent = parentEl ? register(parentEl) : null;
    const id = nodes.length;
    idByEl.set(el, id);

    const tag = (el.localName || '').toLowerCase();
    const role = roleOf(el);
    const style = styleOf(el);
    const onStack = stackSet.has(el);
    const associatedLabel =
      tag === 'label' &&
      (associatedLabels.has(el) ||
        el.control === targetEl ||
        Boolean(
          targetEl.id &&
            el.getAttribute &&
            el.getAttribute('for') === targetEl.id &&
            el.getRootNode &&
            targetEl.getRootNode &&
            el.getRootNode() === targetEl.getRootNode()
        ));
    const modal =
      tag === 'dialog' ||
      role === 'dialog' ||
      role === 'alertdialog' ||
      (el.getAttribute && el.getAttribute('aria-modal') === 'true');
    const landmarkOrChrome = LANDMARK_TAGS.has(tag) || LANDMARK_ROLES.has(role);
    const cursorPointer = Boolean(!rootCursorPointer && style && style.cursor === 'pointer');
    const hasOnclick = Boolean((el.hasAttribute && el.hasAttribute('onclick')) || typeof el.onclick === 'function');
    const clickableContainer =
      !landmarkOrChrome &&
      (CLICKABLE_CONTAINER_ROLES.has(role) ||
        (tag === 'a' && el.hasAttribute && el.hasAttribute('href')) ||
        tag === 'button' ||
        tag === 'label' ||
        tag === 'summary' ||
        hasOnclick ||
        cursorPointer ||
        Boolean(
          el.hasAttribute &&
            (el.hasAttribute('data-href') || el.hasAttribute('data-url') || el.hasAttribute('data-action'))
        ));
    let containedModal = null;
    if (onStack && !modal) {
      let innerModal = el.querySelector ? el.querySelector(MODAL_SELECTOR) : null;
      if (!innerModal && el.parentElement && el.parentElement.querySelector) {
        const parentTag = (el.parentElement.localName || '').toLowerCase();
        if (!LANDMARK_TAGS.has(parentTag) && (!el.parentElement.contains || !el.parentElement.contains(targetEl))) {
          innerModal = el.parentElement.querySelector(MODAL_SELECTOR);
        }
      }
      if (innerModal && innerModal !== targetEl && (!innerModal.contains || !innerModal.contains(targetEl))) {
        const innerRole = roleOf(innerModal);
        containedModal = {
          role: innerRole,
          tag: (innerModal.localName || '').toLowerCase(),
          domId: isSensitive(innerModal) ? '' : String(innerModal.id || '').slice(0, 80),
          name: nameOf(innerModal, innerRole),
        };
      }
    }

    nodes.push({
      id,
      parent,
      tag,
      domId: isSensitive(el) ? '' : String(el.id || '').slice(0, 80),
      role,
      name: nameOf(el, role),
      href: tag === 'a' && el.hasAttribute && el.hasAttribute('href') ? String(el.href || '') : '',
      position: style && style.position ? String(style.position) : 'static',
      transparent: onStack ? isTransparent(el, style) : false,
      interactive: isInteractive(el, tag, role),
      hasInteractiveDescendant: Boolean(onStack && el.querySelector && el.querySelector(INTERACTIVE_SELECTOR)),
      modal,
      landmarkOrChrome,
      clickableContainer,
      cursorPointer,
      associatedLabel,
      rect: rectOf(el),
      containedModal,
    });
    return id;
  }

  const target = register(targetEl);
  const stackIds = stack.map(register);
  return { target, stack: stackIds, nodes };
}
