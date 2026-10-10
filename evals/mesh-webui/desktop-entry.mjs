import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {createCaseRunner} from './case-runner.mjs';
import {checks,validateManifest,matrix} from './model.mjs';
import {deadline,capture} from './browser.mjs';
import {attachDesktop} from './desktop-cdp.mjs';
import {desktopSnapshot,desktopDc} from './desktop-records.mjs';
import {desktopOutput} from './desktop-guard.mjs';

assert.equal(process.env.B5_GO_LIVE,'yes');
const manifest=validateManifest(JSON.parse(await readFile(process.argv[2],'utf8')));
const out=desktopOutput(process.argv[3],process.env.B5_EVIDENCE_ROOT);
const view=process.argv[4],provider=process.argv[5];
assert.ok(['local','remote'].includes(view));assert.ok(['claude','codex','opencode','grok'].includes(provider));
const machine=view==='local'?'windows':'mac',direction=view==='local'?'mac-web-to-windows':'windows-web-to-mac';
const target=manifest[machine].tabs.find(t=>t.provider===provider);assert.ok(target);
const engine=createCaseRunner({manifest,out,directionFilter:direction,providerFilter:provider,promptPad:process.env.B5_PROMPT_PAD,desktopView:view,snapshotRecord:desktopSnapshot});
await mkdir(out,{recursive:false});let attached;
const start=new Date().toISOString();
try {
  for(const check of checks.filter(c=>manifest.notInScopeChecks.includes(c))) await engine.policyReceipt(target,check,'NOT_IN_SCOPE','Owner task_m2rx0n');
  if(target.doNotTouch) {
    const status=machine==='windows'&&provider==='claude'?'BLOCKED_EXTERNAL':'BLOCKED';
    for(const check of checks.filter(c=>!manifest.notInScopeChecks.includes(c))) await engine.policyReceipt(target,check,status,'Fixture excluded; no desktop/native/focus/input');
  } else {
    const proof=await desktopSnapshot(manifest,machine,target);
    assert.equal(proof.binding.after.boot_stage,'agent_ready');assert.equal(proof.binding.after.agent_state.state,'idle');assert.ok(proof.binding.after.pty_id);
    await writeFile(path.join(out,'readiness-native.json'),JSON.stringify(proof,null,2),{flag:'wx'});
    attached=await attachDesktop(manifest,out);
    await capture(attached.page,'desktop-before');
    await deadline(engine.runDesktopCase(attached.page,machine),'Entire B5 desktop case',550000);
  }
} catch(error) {
  await writeFile(path.join(out,'run-failure.json'),JSON.stringify({status:'FAIL',error:String(error.stack??error),at:new Date().toISOString(),rows:engine.rows},null,2),{flag:'wx'});
  process.exitCode=1;
} finally {
  if(attached) await attached.detach();
  await writeFile(path.join(out,'results.md'),matrix(engine.rows));
  await writeFile(path.join(out,'run-boundary.json'),JSON.stringify({started_at:start,ended_at:new Date().toISOString(),view,provider,rows:engine.rows.length},null,2),{flag:'wx'});
}
try {await desktopDc(['tab','list','--json']);}
catch(error) {
  await writeFile(path.join(out,'windows-health-failure.json'),JSON.stringify({at:new Date().toISOString(),error:String(error)},null,2),{flag:'wx'});
  process.exitCode=72;
}
if(engine.rows.some(r=>r.status==='FAIL')) process.exitCode??=1;
