(capture => {
 const state = window[Symbol.for('domios.browser.pointer')];
 if (!state || state.document !== document) return false;
 if (state.observer.takeRecords().length) state.epoch++;
 return state.id === capture.id && state.epoch === capture.epoch;
})
