import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {createHarness,deadline} from './browser.mjs';
import {createCaseRunner} from './case-runner.mjs';
import {snapshot} from './records.mjs';
import {validateManifest,validateOutput,matrix} from './model.mjs';
assert.equal(process.env.B2_GO_LIVE,'yes');
const manifest=validateManifest(JSON.parse(await readFile(process.argv[2],'utf8')));
const out=validateOutput(process.argv[3]);
const tab=manifest.mac.tabs.find(t=>t.provider==='opencode');
assert.equal(tab.tabId,'830bd05b');
assert.equal(tab.doNotTouch,false);
assert.equal(tab.sessionId,'ses_edae5f40bffe1zQiAG7P1Ujtx7');
await mkdir(out,{recursive:true});
const start=new Date().toISOString();
const engine=createCaseRunner({manifest,out,directionFilter:'mac-home-web',providerFilter:'opencode',promptPad:process.env.CODETERM_PAD_DIR,homeWeb:true});
let harness;
try {
  const proof=await snapshot(manifest,'mac',tab);
  await writeFile(path.join(out,'readiness-native.json'),JSON.stringify(proof,null,2),{flag:'wx'});
  assert.equal(proof.binding.after.agent_session_id,tab.sessionId);
  assert.equal(proof.binding.after.agent_lifecycle,'agent_ready');
  assert.equal(proof.binding.after.agent_state,'idle');
  assert.ok(proof.binding.after.pty_id);
  await engine.policyReceipt(tab,'model-switch','NOT_IN_SCOPE','Owner delegated model switch to task_m2rx0n');
  harness=await createHarness(manifest,out);
  await deadline(engine.runHomeWebCase(harness),'Mac OpenCode home-web run',550_000);
} catch(error) {
  await writeFile(path.join(out,'run-failure.json'),JSON.stringify({status:'FAIL',error:String(error.stack??error)},null,2),{flag:'wx'});
  process.exitCode=1;
} finally {
  if(harness) await harness.close();
  await writeFile(path.join(out,'results.md'),matrix(engine.rows));
  await writeFile(path.join(out,'boundaries.json'),JSON.stringify({start,end:new Date().toISOString(),direction:'mac-home-web',tab_id:tab.tabId,session_id:tab.sessionId},null,2),{flag:'wx'});
}
if(engine.rows.some(r=>r.status==='FAIL')) process.exitCode=1;
