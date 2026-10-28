function domiosAtomicAction(epoch, action, text) {
 const state = window[Symbol.for('domios.browser.capture')];
 if (!state || state.document !== document || !this.isConnected) return 'stale';
 if (state.observer.takeRecords().length) state.epoch++;
 if (state.epoch !== epoch) return 'stale';
 if (this.disabled || this.readOnly || this.getAttribute('aria-disabled') === 'true') return 'disabled';
 if (!this.getClientRects().length) return 'hidden';
 if (action === 'click') { this.click(); return 'performed'; }
 const proto = this instanceof HTMLInputElement ? HTMLInputElement.prototype : this instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : null;
 const setter = proto && Object.getOwnPropertyDescriptor(proto, 'value').set;
 if (!setter) return 'unsupported_input';
 this.focus(); setter.call(this, text);
 this.dispatchEvent(new Event('input', {bubbles:true}));
 this.dispatchEvent(new Event('change', {bubbles:true}));
 return 'performed';
}
