function domiosTypeFocus(secret) {
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
 this.focus();
 if (this.ownerDocument.activeElement !== this && this.getRootNode().activeElement !== this) return 'focus_failed';
 if (proto) Object.getOwnPropertyDescriptor(proto, 'value').set.call(this, '');
 else this.textContent = '';
 this.dispatchEvent(new Event('input', {bubbles:true}));
 if (!this.isConnected || (this.ownerDocument.activeElement !== this && this.getRootNode().activeElement !== this)) return 'focus_lost';
 return 'focused';
}
