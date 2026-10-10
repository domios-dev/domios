import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHarness,switchPeer,selectTab,capture,observe,activity,waitDom,deadline,captureTerminalView} from './browser.mjs';
import {validateManifest,validateOutput,cheap,claim,matrix,convergence,checks} from './model.mjs';
import {snapshot} from './records.mjs';
import {selectedChatSurface} from './chat-selection.mjs';
const pad = process.env.CODETERM_PAD_DIR;
assert.ok(pad, 'B2 authored provider prompts require the calling agent pad');
const manifest = validateManifest(JSON.parse(await readFile(process.argv[2],'utf8')));
const out = validateOutput(process.argv[3]);
const directionFilter=process.env.B2_DIRECTION;
const providerFilter=process.env.B2_PROVIDER;
assert.ok(['mac-web-to-windows','windows-web-to-mac'].includes(directionFilter));
assert.ok(['claude','codex','opencode','grok'].includes(providerFilter));
const rows = [];
let serial = 0;
let harness;
async function persist(name,value) {
  await writeFile(path.join(out,name),JSON.stringify(value,null,2),{flag:'wx'});
  return name;
}
async function resolvedId(page, tab, targetPeer) {
  assert.notEqual(tab.doNotTouch, true, 'Protected fixture cannot be selected');
  await selectTab(page,tab.tabId,{workspaceId:tab.workspaceId});
  const candidates = [tab.tabId,`mesh:${targetPeer}/${tab.tabId}`];
  const ids = await page.locator('[data-testid="tab-item"]').evaluateAll(nodes=>nodes.map(e=>e.getAttribute('data-tab-id')));
  const matches = candidates.filter(id=>ids.includes(id));
  assert.equal(matches.length,1,'Exactly one rendered home/peer-qualified fixture identity required');
  return matches[0];
}
async function check(page,direction,tab,name,fn) {
  const label = `${++serial}-${direction}-${tab.provider}-${name}`;
  let status='PASS', details, error;
  try { details = await deadline(fn(),name); }
  catch (cause) {status='FAIL';error=String(cause.stack ?? cause);}
  let screen;
  try { screen = await capture(page,label); }
  catch (cause) {status='FAIL';error=(error ?? '')+'\nCapture failed: '+cause;}
  const receipt={provider:tab.provider,check:name,view:'web UI',direction,status,
    tab_id:tab.tabId,session_id:tab.sessionId,details,error,screenshot:screen?.screenshot,
    steps:page.__b2Steps ?? [],expected:name+' completes with visible far-end and exact session evidence',
    observed:screen?.metadata,log_lines:page.__b2Errors ?? []};
  receipt.evidence=await persist(label+'-receipt.json',receipt);
  rows.push(receipt);
  await writeFile(path.join(out,'receipts.json'),JSON.stringify(rows,null,2));
  await writeFile(path.join(out,'results.md'),matrix(rows));
  if (status==='FAIL') return null;
  return details ?? true;
}
async function native(machine,tab,label) {
  const proof = await snapshot(manifest,machine,tab);
  const evidence = await persist(`${++serial}-${label}-native.json`,proof);
  return {proof,evidence};
}
async function promptFile(label,prompt) {
  const directory=path.join(pad,'prompts');
  await mkdir(directory,{recursive:true});
  const filename=path.join(directory,label+'.txt');
  await writeFile(filename,prompt,{flag:'wx'});
  return readFile(filename,'utf8');
}
async function submit(page,prompt,label) {
  const text=await promptFile(label,prompt);
  page.__b2Steps.push({action:'composer send',promptFile:'prompts/'+label+'.txt',at:Date.now()});
  const chat = await selectedChatSurface(page);
  await chat.getByTestId('chat-composer').fill(text);
  await chat.getByTestId('composer-send-button').click();
  await capture(page,label+'-submitted');
}
async function turn(page,home,machine,tab,uiId,index,direction) {
  const before=await native(machine,tab,'before-turn-'+direction+'-'+index);
  const token=`B2_${path.basename(out)}_${direction}_${tab.provider}_${index}`;
  const prompt=index===0
    ? `${token}\nUse one read-only directory-listing tool in your current test folder, then respond with ${token} and a short fenced JavaScript code block. Do not edit files or invoke CodeTerm.`
    : `${token}\nReply with exactly ${token}.`;
  await observe(page); await observe(home);
  await submit(page,prompt,token);
  await waitDom(page,{selector:`[data-testid="tab-session-chat"][data-tab-id="${uiId}"] [data-testid="agent-state"]`,attr:'data-agent-state',value:'working'});
  await capture(page,token+'-working');
  await waitDom(page,{selector:`[data-testid="tab-session-chat"][data-tab-id="${uiId}"] [data-testid="agent-state"]`,attr:'data-agent-state',value:'idle'});
  const after=await native(machine,tab,'after-turn-'+direction+'-'+index);
  const receipt=claim(after.proof,tab,prompt,before.proof.records.length);
  const assistant=receipt.assistant.map(r=>r.text).join('');
  assert.ok(assistant.includes(token),'Persisted assistant must answer supplied marker');
  await waitDom(page,{selector:'[data-testid="chat-message"][data-message-role="assistant"]',text:token});
  const evidence=await capture(page,token+'-done');
  return {token,prompt,provider_record:after.evidence,user:receipt.user,
    assistant:receipt.assistant,screen:evidence,web:await activity(page),home:await activity(home),
    newRecords:after.proof.records.slice(before.proof.records.length)};
}
async function provider(page,home,direction,machine,tab) {
  const uiId=await resolvedId(page,tab,manifest[machine].peerId);
  await selectTab(page,uiId); await selectTab(home,tab.tabId);
  const initial=await native(machine,tab,'initial-'+direction+'-'+tab.provider);
  const actual=[...initial.proof.records].reverse().find(r=>r.role==='assistant'&&r.model)?.model ?? initial.proof.session_model;
  assert.ok(cheap(tab.provider,actual),'Initial provider-owned actual model must be cheap before traffic');
  page.__b2Steps=[{action:'titlebar peer switch',target:manifest[machine].peerId}, {action:'select tab',tabId:uiId,sessionId:tab.sessionId}];
  page.__b2Errors=[];
  await check(page,direction,tab,'tab-list',async()=>{
    assert.ok(await page.locator(`[data-testid="tab-item"][data-tab-id="${uiId}"]`).isVisible());
    return {nativeRecord:initial.evidence,uiId};
  });
  await check(page,direction,tab,'managed-by',async()=>{
    assert.ok(tab.controllerTitle,'Fixture must have an explicit controller title');
    await waitDom(page,{selector:`[data-testid="tab-item"][data-tab-id="${uiId}"] [data-testid="remote-managed-by"]`,attr:'aria-label',value:`managed by ${tab.controllerTitle}`});
    return {controller:tab.controllerTitle,uiId};
  });
  const turns=[];
  for (let i=0;i<15;i++) {
    const result=await check(page,direction,tab,'composer-send',()=>turn(page,home,machine,tab,uiId,i,direction));
    if (!result) return;
    turns.push(result);
  }
  await check(page,direction,tab,'medium-chat-15-30',async()=>{
    assert.equal(turns.length,15); assert.equal(new Set(turns.map(t=>t.user.locator)).size,15);
    return {providerRecords:turns.map(t=>t.provider_record)};
  });
  await check(page,direction,tab,'chat-render',async()=>{
    for(const t of turns) await waitDom(page,{selector:'[data-testid="chat-message"][data-message-role="assistant"]',text:t.token});
    return {markers:turns.map(t=>t.token),providerRecords:turns.map(t=>t.provider_record)};
  });
  await check(page,direction,tab,'status-working',async()=>{
    assert.ok(turns.every(t=>t.web.some(e=>e.state==='working')));return turns.map(t=>t.web);
  });
  await check(page,direction,tab,'status-done',async()=>{
    assert.ok(turns.every(t=>t.web.at(-1)?.state==='idle'));return turns.map(t=>t.web);
  });
  await check(page,direction,tab,'sync-within-1s',async()=>{
    return turns.map(convergence);
  });
  await check(page,direction,tab,'progress-live',async()=>{
    assert.ok(turns.some(t=>t.web.some(e=>e.sending)),'Sending dot never observed');
    assert.ok(turns.some(t=>new Set(t.web.filter(e=>e.state==='working').map(e=>e.progress)).size>1),'Progress did not update live');
    assert.ok(turns.some(t=>new Set(t.web.filter(e=>e.state==='working').map(e=>e.elapsed).filter(Boolean)).size>1),'Elapsed time did not update live');
    return turns.map(t=>t.web);
  });
  await check(page,direction,tab,'tool-rows',async()=>{
    assert.ok(turns[0].newRecords.some(r=>r.tool),'No persisted tool call in supplied turn');
    await waitDom(page,{selector:'[data-testid="chat-tool-call"]'});return {providerRecord:turns[0].provider_record};
  });
  await check(page,direction,tab,'code-blocks',async()=>{
    assert.ok(turns[0].assistant.some(r=>r.text.includes('```')),'No persisted code fence');
    await waitDom(page,{selector:'[data-testid="chat-code-block"]'});return {providerRecord:turns[0].provider_record};
  });
  await check(page,direction,tab,'attachments',async()=>{
    const token=`B2_attachment_${path.basename(out)}_${direction}_${tab.provider}`;
    const file=path.join(pad,'prompts',token+'.txt');
    await writeFile(file,token,{flag:'wx'});
    const before=await native(machine,tab,'before-attachment');
    await page.getByTestId('composer-attach-input').setInputFiles(file);
    await waitDom(page,{selector:'[data-testid="attachment-chip"]',text:token});
    await submit(page,`Read the attached text and reply with its exact contents.`,token+'-prompt');
    await waitDom(page,{selector:'[data-testid="chat-message"][data-message-role="assistant"]',text:token});
    await waitDom(page,{selector:'[data-testid="agent-state"]',attr:'data-agent-state',value:'idle'});
    const after=await native(machine,tab,'after-attachment');
    const delta=after.proof.records.slice(before.proof.records.length);
    assert.ok(delta.some(r=>r.role==='user'&&(r.attachment||r.text.includes(token))),'Attachment absent from own-session input');
    assert.ok(delta.some(r=>r.role==='assistant'&&r.text.includes(token)),'Attachment contents were not answered in own session');
    await waitDom(page,{selector:'[data-testid="chat-inline-attachment"]'});
    return {providerRecord:after.evidence};
  });
  await check(page,direction,tab,'stop',async()=>{
    const before=await native(machine,tab,'before-stop');
    const token=`B2_stop_${path.basename(out)}_${direction}_${tab.provider}`;
    await submit(page,`${token}\nGenerate 1500 numbered unique short lines beginning with ${token}.`,token);
    await waitDom(page,{selector:'[data-testid="agent-state"]',attr:'data-agent-state',value:'working'});
    await capture(page,token+'-before-stop');
    page.__b2Steps.push({action:'click Stop',at:Date.now()});
    await page.getByTestId('agent-stop').click();
    await waitDom(page,{selector:'[data-testid="agent-state"]',attr:'data-agent-state',value:'idle'});
    const after=await native(machine,tab,'after-stop');
    const delta=after.proof.records.slice(before.proof.records.length);
    assert.ok(delta.some(r=>r.role==='user'&&r.text.startsWith(token)),'Stop turn not persisted');
    assert.ok(delta.some(r=>r.cancelled === true),'Own-session cancellation record missing');
    return {providerRecord:after.evidence};
  });
  if (!manifest.notInScopeChecks?.includes('model-switch')) await check(page,direction,tab,'model-switch',async()=>{
    assert.ok(tab.switchModel&&tab.switchModel!==actual,'Distinct permitted cheap model required');
    assert.ok(cheap(tab.provider,tab.switchModel));
    await page.getByTestId('status-model-trigger').click();
    const option=page.locator(`[data-testid="status-model-picker"] [role="option"][title="${tab.switchModel}"]`);
    assert.notEqual(await option.getAttribute('aria-disabled'),'true');
    page.__b2Steps.push({action:'select model',model:tab.switchModel,at:Date.now()});
    await option.click();
    const before=await native(machine,tab,'before-model-probe');
    const token=`B2_model_${path.basename(out)}_${direction}_${tab.provider}`;
    const prompt=`${token}\nReply exactly ${token}.`;
    await submit(page,prompt,token);
    await waitDom(page,{selector:'[data-testid="chat-message"][data-message-role="assistant"]',text:token});
    await waitDom(page,{selector:'[data-testid="agent-state"]',attr:'data-agent-state',value:'idle'});
    const after=await native(machine,tab,'after-model-probe');
    const receipt=claim(after.proof,tab,prompt,before.proof.records.length,tab.switchModel);
    return {providerRecord:after.evidence,model:tab.switchModel,receipt};
  });
  await check(page,direction,tab,'history-reload',async()=>{
    const selectedPeer=await page.evaluate(()=>localStorage.getItem(`codeterm-web-relay-peer:${location.origin}`));
    page.__b2Steps.push({action:'reload isolated browser page',at:Date.now()});
    await page.reload({waitUntil:'domcontentloaded'});
    assert.equal(await page.evaluate(()=>localStorage.getItem(`codeterm-web-relay-peer:${location.origin}`)),selectedPeer);
    await selectTab(page,uiId);
    for(const t of turns) await waitDom(page,{selector:'[data-testid="chat-message"][data-message-role="assistant"]',text:t.token});
    return {providerRecords:turns.map(t=>t.provider_record),selectedPeer};
  });
}
async function runDirection(gateway,machine,direction) {
  const {page}=await harness.openGateway(gateway);
  const {page:home}=await harness.openGateway(machine);
  page.__b2Steps=[];page.__b2Errors=[];
  page.on('pageerror',error=>page.__b2Errors.push({at:Date.now(),error:String(error)}));
  await capture(page,direction+'-gateway-baseline');
  await page.getByTestId('peer-picker-button').click();
  await capture(page,direction+'-picker-menu');
  await page.getByTestId('peer-picker-button').click();
  let switched=false;
  for(const tab of manifest[machine].tabs.filter(t=>t.provider===providerFilter)) {
    if(!switched) {
      const result=await check(page,direction,tab,'peer-switch',async()=>{
        const receipt=await switchPeer(page,manifest[machine].peerId);
        assert.equal(new URL(page.url()).origin,manifest[gateway].origin);
        const id=await resolvedId(page,tab,manifest[machine].peerId);
        await selectTab(page,id);return receipt;
      });
      if(!result) break;
      switched=true;
    } else await check(page,direction,tab,'peer-switch',async()=>{
      const id=await resolvedId(page,tab,manifest[machine].peerId);await selectTab(page,id);
      return {origin:new URL(page.url()).origin,peer:manifest[machine].peerId};
    });
    try {await provider(page,home,direction,machine,tab);}
    catch(error) {await check(page,direction,tab,'chat-render',async()=>{throw error;});}
  }
  for(const terminal of manifest[machine].terminals ?? []) {
    await captureTerminalView(page,await resolvedId(page,terminal,manifest[machine].peerId),`${direction}-terminal-${terminal.tabId}`);
  }
  for(const tab of manifest[machine].tabs.filter(t=>t.provider===providerFilter && !t.doNotTouch)) {
    await check(page,direction,tab,'gateway-return',async()=>{
      await switchPeer(page,null);assert.equal(new URL(page.url()).origin,manifest[gateway].origin);
      const returned = manifest[gateway].tabs.find(t=>t.provider===providerFilter && !t.doNotTouch)
        ?? manifest[gateway].shells?.find(t=>!t.doNotTouch);
      assert.ok(returned,'Gateway needs an authorized return fixture');
      await selectTab(page,returned.tabId,{mode:returned.provider ? 'chat' : 'terminal',workspaceId:returned.workspaceId});
      return {gateway,origin:manifest[gateway].origin,returnedTabId:returned.tabId};
    });
  }
}
async function policyReceipt(tab, name, status, reason) {
  await mkdir(out,{recursive:true});
  const receipt={provider:tab.provider,check:name,view:'web UI',direction:directionFilter,status,
    tab_id:tab.tabId,session_id:tab.sessionId,expected:'Only permitted in-scope fixtures receive actions',
    observed:reason,steps:[],owner_task:status==='NOT_IN_SCOPE' ? 'task_m2rx0n' : 'task_n6bp2e'};
  receipt.evidence=await persist(`${++serial}-policy-${name}.json`,receipt);
  rows.push(receipt);
  await writeFile(path.join(out,'receipts.json'),JSON.stringify(rows,null,2));
}
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
      if(directionFilter==='mac-web-to-windows') await runDirection('mac','windows',directionFilter);
      else await runDirection('windows','mac',directionFilter);
    };
    await deadline(run(),'Entire B2 live run',550_000);
  }
} catch(error) {
  await mkdir(out,{recursive:true});
  await persist('run-failure.json',{status:'FAIL',error:String(error.stack??error),rows});
  process.exitCode=1;
} finally {
  if(harness) await harness.close();
  await writeFile(path.join(out,'results.md'),matrix(rows));
}
if(rows.some(r=>r.status==='FAIL')) process.exitCode=1;
