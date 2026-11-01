(() => {
 const key = Symbol.for('domios.browser.capture');
 let state = window[key];
 if (!state) {
   state = { epoch: 0, document };
   state.observer = new MutationObserver(() => state.epoch++);
   state.observer.observe(document, {subtree:true, childList:true, attributes:true, characterData:true});
   for (const event of ['input','pointerdown','keydown']) document.addEventListener(event, () => state.epoch++, true);
   window[key] = state;
 }
 if (state.observer.takeRecords().length) state.epoch++;
 return state.epoch;
})()
