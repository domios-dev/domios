import assert from 'node:assert/strict';
import path from 'node:path';
export const DESKTOP_URL='http://127.0.0.1:1420/';
export const CDP_URL='http://127.0.0.1:9223';
export function desktopTarget(targets) {
  const pages=targets.filter(t=>t.type==='page');
  assert.equal(pages.length,1,'CDP endpoint must contain exactly one page, no other window');
  assert.equal(pages[0].url,DESKTOP_URL,'Only the exact DEV desktop page is permitted');
  assert.ok(pages[0].id&&pages[0].webSocketDebuggerUrl);
  const socket=new URL(pages[0].webSocketDebuggerUrl);
  assert.equal(socket.protocol,'ws:');assert.ok(['127.0.0.1','localhost'].includes(socket.hostname));
  assert.equal(socket.port,'9223');
  return pages[0];
}
export function desktopOutput(folder,root) {
  assert.ok(root&&path.isAbsolute(root));
  const resolved=path.resolve(folder),approved=path.resolve(root);
  assert.equal(path.dirname(resolved),approved,'Desktop evidence must be direct child of the approved Hopper evidence root');
  assert.match(path.basename(resolved),/^b5-[a-zA-Z0-9_.-]+$/);
  return resolved;
}
export function refuseDesktopLifecycle(page) {
  for(const name of ['goto','reload','close','setContent','goBack','goForward']) {
    page[name]=async()=>{throw Error('Desktop '+name+' is forbidden');};
  }
  const context=page.context();
  context.close=async()=>{throw Error('Desktop context close is forbidden');};
  context.newPage=async()=>{throw Error('Additional desktop page is forbidden');};
}
