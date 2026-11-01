(function domiosAccessibleWait(id, duration) {
 const key = Symbol.for('domios.browser.wait');
 let state = window[key];
 if (!state) {
   state = { waiters: new Map() };
   const wake = () => {
     for (const [id, deadline] of state.waiters) if (deadline <= performance.now()) state.waiters.delete(id);
     if (state.waiters.size && typeof window.__domiosAXWake === 'function') window.__domiosAXWake('changed');
   };
   state.observer = new MutationObserver(wake);
   state.observer.observe(document, { subtree:true, childList:true, attributes:true, characterData:true });
   for (const event of ['input','change','toggle','focusin']) document.addEventListener(event, wake, true);
   window[key] = state;
 }
 for (const [id, deadline] of state.waiters) if (deadline <= performance.now()) state.waiters.delete(id);
 if (state.waiters.size >= 128) throw new Error('wait_limit: too many active waits');
 state.waiters.set(id, performance.now() + duration);
})
