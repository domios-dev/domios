import assert from 'node:assert/strict';
import {createRequire} from 'node:module';
import {mkdir,writeFile} from 'node:fs/promises';
import path from 'node:path';
import {desktopTarget,refuseDesktopLifecycle,CDP_URL,DESKTOP_URL} from './desktop-guard.mjs';
import {deadline} from './browser.mjs';
import {armBrowserDiagnostics} from './browser-diagnostics.mjs';

export async function attachDesktop(manifest,out) {
  assert.equal(process.platform,'win32','Windows utility shell execution only');
  assert.equal(process.env.B5_GO_LIVE,'yes');
  const require=createRequire(import.meta.url);
  const {chromium}=require('C:/Developer/codeterm/codeterm/node_modules/.pnpm/playwright-core@1.60.0/node_modules/playwright-core/index.js');
  const response=await fetch(CDP_URL+'/json',{signal:AbortSignal.timeout(30000),redirect:'error'});
  assert.equal(response.status,200);const target=desktopTarget(await response.json());
  await mkdir(out,{recursive:true});
  await writeFile(path.join(out,'desktop-target.json'),JSON.stringify({at:new Date().toISOString(),id:target.id,url:target.url,endpoint:CDP_URL},null,2),{flag:'wx'});
  const browser=await chromium.connectOverCDP(CDP_URL,{timeout:30000,noDefaults:true});
  let diagnostics,page;
  try {
    const pages=browser.contexts().flatMap(c=>c.pages());
    assert.equal(pages.length,1);[page]=pages;assert.equal(page.url(),DESKTOP_URL);
    refuseDesktopLifecycle(page);page.setDefaultTimeout(30000);
    const evaluate=page.evaluate.bind(page);
    page.evaluate=(...args)=>deadline(evaluate(...args),'Desktop protocol evaluate');
    page.__b2Out=out;page.__b2Manifest=manifest;page.__b2Gateway='windows';
    diagnostics=await deadline(armBrowserDiagnostics(page,path.join(out,'diagnostics')),'Desktop diagnostics');
    return {page,async detach() {
      await deadline(diagnostics.finish(),'Desktop diagnostics finish');
      await deadline(browser.close(),'CDP transport disconnect');
    }};
  } catch(error) {
    if(diagnostics) await deadline(diagnostics.finish('attach-failed'),'Desktop diagnostic failure finish');
    await deadline(browser.close(),'CDP failure transport disconnect');throw error;
  }
}
