import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {createHarness,deadline} from './browser.mjs';
import {createCaseRunner} from './case-runner.mjs';
import {validateManifest,validateOutput,matrix,checks} from './model.mjs';
const pad = process.env.CODETERM_PAD_DIR;
assert.ok(pad, 'B2 authored provider prompts require the calling agent pad');
const manifest = validateManifest(JSON.parse(await readFile(process.argv[2],'utf8')));
const out = validateOutput(process.argv[3]);
const directionFilter=process.env.B2_DIRECTION;
const providerFilter=process.env.B2_PROVIDER;
assert.ok(['mac-web-to-windows','windows-web-to-mac'].includes(directionFilter));
assert.ok(['claude','codex','opencode','grok'].includes(providerFilter));
const engine=createCaseRunner({manifest,out,directionFilter,providerFilter,promptPad:pad});
const {rows,policyReceipt}=engine;
let harness;
try {
  const machine=directionFilter==='mac-web-to-windows' ? 'windows' : 'mac';
  const target=manifest[machine].tabs.find(tab=>tab.provider===providerFilter);
  assert.ok(target,'Selected provider fixture is required');
  for(const name of checks.filter(name=>manifest.notInScopeChecks?.includes(name))) {
    await policyReceipt(target,name,'NOT_IN_SCOPE','Owner delegated model-switch to task_m2rx0n');
  }
  if(target.doNotTouch) {
    for(const name of checks.filter(name=>!manifest.notInScopeChecks?.includes(name))) {
      await policyReceipt(target,name,'BLOCKED','Windows OpenCode fixture is DO_NOT_TOUCH/task_n6bp2e; no browser, focus, native read or input attempted');
    }
  } else {
    harness=await createHarness(manifest,out);
    const run=async()=>{
      await engine.runWebCase(harness);
    };
    await deadline(run(),'Entire B2 live run',550_000);
  }
} catch(error) {
  await mkdir(out,{recursive:true});
  await writeFile(path.join(out,'run-failure.json'),JSON.stringify({status:'FAIL',error:String(error.stack??error),rows},null,2),{flag:'wx'});
  process.exitCode=1;
} finally {
  if(harness) await harness.close();
  await writeFile(path.join(out,'results.md'),matrix(rows));
}
if(rows.some(r=>r.status==='FAIL')) process.exitCode=1;
