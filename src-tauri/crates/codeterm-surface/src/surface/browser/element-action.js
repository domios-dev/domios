function domiosElementAction(epoch, action, value) {
 const state = window[Symbol.for('domios.browser.capture')];
 if (!state || state.document !== document || !this.isConnected) return 'stale';
 if (state.observer.takeRecords().length) state.epoch++;
 if (state.epoch !== epoch) return 'stale';
 if (this.disabled || this.readOnly || this.getAttribute('aria-disabled') === 'true') return 'disabled';
 if (!this.getClientRects().length) return 'hidden';
 if (action === 'scroll') { this.scrollIntoView({block:'center',inline:'nearest',behavior:'instant'}); return 'performed'; }
 if (this instanceof HTMLInputElement && this.type === 'checkbox') {
   if (!['true','false'].includes(value)) return 'invalid_checkbox_value';
   if (this.checked !== (value === 'true')) this.click();
   return this.checked === (value === 'true') ? 'performed' : 'state_unconfirmed';
 }
 if (this instanceof HTMLSelectElement) {
   const byValue = [...this.options].filter(option => option.value === value);
   const matches = byValue.length ? byValue : [...this.options].filter(option => option.text === value);
   if (matches.length !== 1) return matches.length ? 'ambiguous' : 'not_found';
   if (matches[0].disabled || matches[0].parentElement.disabled) return 'disabled';
   Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,'value').set.call(this,matches[0].value);
   this.dispatchEvent(new Event('input',{bubbles:true}));
   this.dispatchEvent(new Event('change',{bubbles:true}));
   return 'performed';
 }
 return 'unsupported_input';
}
