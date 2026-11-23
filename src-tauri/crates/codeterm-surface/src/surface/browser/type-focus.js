function domiosTypeFocus(secret, phase) {
 if (!this.isConnected || this.closest('[hidden], [inert]')) return 'hidden';
 const style = getComputedStyle(this);
 if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse' || !this.getClientRects().length) return 'hidden';
 if (this.disabled || this.readOnly || this.getAttribute('aria-disabled') === 'true') return 'disabled';
 const proto = this instanceof HTMLInputElement ? HTMLInputElement.prototype : this instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : null;
 if (!proto && !this.isContentEditable) return 'unsupported_input';
 if (secret) {
  if (!(this instanceof HTMLInputElement)) return 'unmaskable';
  this.type='password';
  if (this.type !== 'password') return 'unmaskable';
 }
 const doc = this.ownerDocument;
 const root = this.getRootNode();
 const isFocused = () => {
  const active = root.activeElement || doc.activeElement;
  return Boolean(this.isConnected && (active === this || (active && this.contains(active))));
 };
 const selection = () => (root.getSelection && root.getSelection()) || doc.getSelection();
 const placeCaret = () => {
  const sel = selection();
  if (!sel) return false;
  const range = doc.createRange();
  const br = Array.from(this.querySelectorAll('br')).find(node => node.parentNode && node.parentNode.isContentEditable);
  if (br && br.parentNode) {
   const idx = Array.prototype.indexOf.call(br.parentNode.childNodes, br);
   range.setStart(br.parentNode, idx);
  } else {
   const walker = doc.createTreeWalker(this, NodeFilter.SHOW_TEXT);
   let textNode = null;
   while (walker.nextNode()) {
    const candidate = walker.currentNode;
    if (!candidate.parentNode || !candidate.parentNode.isContentEditable) continue;
    if (/[\u200B\uFEFF\u2060]/.test(candidate.data) || !/^[\t\n\r ]*$/.test(candidate.data)) {
     textNode = candidate;
     break;
    }
   }
   if (textNode) {
    range.setStart(textNode, textNode.length);
   } else {
    let leaf = this;
    while (leaf.firstElementChild && leaf.firstElementChild.isContentEditable && !/^(BR|IMG|HR|INPUT|TEXTAREA)$/.test(leaf.firstElementChild.tagName)) {
     leaf = leaf.firstElementChild;
    }
    range.setStart(leaf, 0);
   }
  }
  range.collapse(true);
  sel.removeAllRanges();
  sel.addRange(range);
  return true;
 };
 if (phase === 'caret') {
  if (!isFocused()) this.focus();
  if (!isFocused()) return 'focus_lost';
  const sel = selection();
  if (!sel) return 'focus_failed';
  const inside = sel.rangeCount > 0 && this.contains(sel.anchorNode) && this.contains(sel.focusNode) && (sel.anchorNode !== this || !this.firstElementChild);
  if (!inside && !placeCaret()) return 'focus_failed';
  return isFocused() ? 'focused' : 'focus_lost';
 }
 this.focus();
 if (!isFocused()) return 'focus_failed';
 if (proto) {
  Object.getOwnPropertyDescriptor(proto, 'value').set.call(this, '');
  this.dispatchEvent(new Event('input', {bubbles:true}));
  return isFocused() ? 'focused' : 'focus_lost';
 }
 const sel = selection();
 if (!sel) return 'focus_failed';
 const rendered = typeof this.innerText === 'string' ? this.innerText : (this.textContent || '');
 const visibleText = rendered.replace(/[\u200B\uFEFF\u2060]/g, '').replace(/^\r?\n$/, '');
 const hasNonText = Boolean(this.querySelector('img, hr, [contenteditable="false"]'));
 const hasContent = visibleText.length > 0 || hasNonText;
 if (hasContent) {
  const range = doc.createRange();
  const walker = doc.createTreeWalker(this, NodeFilter.SHOW_TEXT);
  let firstText = null;
  let lastText = null;
  while (walker.nextNode()) {
   const node = walker.currentNode;
   if (!node.parentNode || !node.parentNode.isContentEditable) continue;
   const formattingOnly = /^[\t\n\r ]+$/.test(node.data) && node.parentNode.children && node.parentNode.children.length > 0;
   if (!formattingOnly && node.data.length > 0) {
    if (!firstText) firstText = node;
    lastText = node;
   }
  }
  if (firstText && lastText && !hasNonText) {
   range.setStart(firstText, 0);
   range.setEnd(lastText, lastText.length);
  } else {
   range.selectNodeContents(this);
  }
  sel.removeAllRanges();
  sel.addRange(range);
  return isFocused() ? 'selected' : 'focus_lost';
 }
 if (!placeCaret()) return 'focus_failed';
 return isFocused() ? 'focused' : 'focus_lost';
}
