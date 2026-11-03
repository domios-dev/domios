((operation, token) => {
 const key = Symbol.for('domios.browser.drag');
 const previous = window[key];
 if (operation === 'arm') {
   if (previous) return false;
   const state = {token, document, started:false, dropped:false, released:false};
   state.listen = event => {
     if (!event.isTrusted) return;
     if (event.type === 'dragstart') state.started = true;
     if (event.type === 'drop') state.dropped = true;
     if (event.type === 'mouseup') state.released = true;
   };
   for (const type of ['dragstart','drop','mouseup']) document.addEventListener(type,state.listen,true);
   window[key] = state;
   return true;
 }
 if (!previous || previous.token !== token || previous.document !== document) return null;
 if (operation === 'clear') {
   for (const type of ['dragstart','drop','mouseup']) document.removeEventListener(type,previous.listen,true);
   delete window[key];
   return true;
 }
 return {started:previous.started,dropped:previous.dropped,released:previous.released};
})
