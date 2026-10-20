export function rendererCensus(root, browser) {
  const terminals = [...root.querySelectorAll('.terminal-xterm-host')].map(host => {
    const term = host.__codetermTerminal;
    return {tab:host.closest('[data-tab-id]')?.getAttribute('data-tab-id')??null,
      normalLines:term?.buffer.normal.length??null,alternateLines:term?.buffer.alternate.length??null,
      activeLines:term?.buffer.active.length??null,cols:term?.cols??null,rows:term?.rows??null};
  });
  const chats = [...root.querySelectorAll('[data-testid="tab-session-chat"]')].map(chat => ({
    tab:chat.getAttribute('data-tab-id'),elements:chat.querySelectorAll('*').length,
    rows:chat.querySelectorAll('[data-source-message-id]').length,
  }));
  const canvas = [...root.querySelectorAll('canvas')];
  const images = [...root.querySelectorAll('img')];
  return {elements:root.querySelectorAll('*').length,chatRows:root.querySelectorAll('[data-source-message-id]').length,
    chatRoots:chats.length,chats,xterms:root.querySelectorAll('.xterm').length,terminals,
    terminalApisAvailable:terminals.filter(t=>t.normalLines!==null).length,
    canvases:canvas.length,canvasBackingPixels:canvas.reduce((n,c)=>n+c.width*c.height,0),
    images:images.length,imageIntrinsicPixels:images.reduce((n,i)=>n+i.naturalWidth*i.naturalHeight,0),
    callbacks:browser.__TAURI_INTERNALS__?.callbacks?.size??null,
    userAgentMemory:{available:typeof browser.performance?.measureUserAgentSpecificMemory==='function',
      crossOriginIsolated:browser.crossOriginIsolated===true,status:'not-collected-to-preserve-minimal-instrumentation'}};
}

export const CENSUS = `(${rendererCensus.toString()})(document,window)`;
