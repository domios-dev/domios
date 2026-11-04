function domiosSubmit(stage, key) {
 const doc = this.ownerDocument;
 const slot = Symbol.for('domios.browser.submit');
 if (stage === 'begin') {
  if (this.ownerDocument.activeElement !== this && this.getRootNode().activeElement !== this) return 'focus_lost';
  const previous = doc[slot];
  if (previous) doc.removeEventListener('submit', previous.listener, true);
  const state = {target:this, key, form:this.form, url:location.href, observed:false};
  state.listener = event => { if (event.target === state.form) state.observed = true; };
  doc[slot] = state;
  doc.addEventListener('submit', state.listener, true);
  return 'ready';
 }
 const state = doc[slot];
 if (!state || state.key !== key || state.target !== this) return 'missing';
 if (stage === 'cleanup') {
  doc.removeEventListener('submit', state.listener, true);
  delete doc[slot];
  return 'cleaned';
 }
 if (state.observed) return 'submitted';
 if (state.url !== location.href) return 'navigated';
 if (!this.isConnected || (this.ownerDocument.activeElement !== this && this.getRootNode().activeElement !== this)) return 'focus_lost';
 if (!state.form || this.form !== state.form || !state.form.isConnected) return 'no_form';
 HTMLFormElement.prototype.requestSubmit.call(state.form);
 return state.observed ? 'submitted' : 'submit_unconfirmed';
}
