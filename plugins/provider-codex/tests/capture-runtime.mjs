import {spawn,execFileSync} from "node:child_process";
import {mkdtempSync,writeFileSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import {join,resolve} from "node:path";
import {once} from "node:events";
import {createServer} from "node:net";
import {createInterface} from "node:readline";

const destination = process.argv[2];
const scenarios = process.argv.slice(3);
if (!destination) throw new Error("Usage: node capture-runtime.mjs <output-directory>");
const model = "gpt-6-luna";
const notifications = new Set(["thread/status/changed","thread/settings/updated","thread/tokenUsage/updated","turn/started","turn/completed","item/started","item/completed","item/agentMessage/delta","item/reasoning/textDelta","item/reasoning/summaryTextDelta","item/reasoning/summaryPartAdded","item/fileChange/patchUpdated","serverRequest/resolved","error"]);
const requests = new Set(["item/tool/requestUserInput","item/commandExecution/requestApproval","item/fileChange/requestApproval"]);
const settings = {model,effort:"low",mode:"approval-required"};
const environment = Object.fromEntries(Object.entries(process.env).filter(([key])=>!key.startsWith("CODETERM") && !["CODEX_THREAD_ID","CODEX_INTERNAL_ORIGINATOR_OVERRIDE"].includes(key)));

async function capture(scenario) {
  const cwd = mkdtempSync(join(tmpdir(),"runtime-capture-"));
  const background = scenario === "background-terminals";
  const controls=new Set();
  const control=background ? createServer(socket=>{controls.add(socket);socket.on("close",()=>controls.delete(socket));}) : undefined;
  if(control) {control.listen(0,"127.0.0.1");await once(control,"listening");}
  const ctx = {epoch:1,cwd,hostVersion:"1.14.7",tabId:"capture-tab",bindingNonce:"capture-nonce",settings,providerSettings:{},claimedSends:[],activeTurns:[],threadParents:[]};
  const fixture={schemaVersion:1,captured:{cliVersion:execFileSync("codex",["--version"],{encoding:"utf8"}).trim(),model,scenario},context:structuredClone(ctx),operations:[],rpc:[],expected:{results:[],events:[]}};
  const started=performance.now();
  const record=entry=>fixture.rpc.push({...entry,receiptMs:Number((performance.now()-started).toFixed(3)),...("frame" in entry ? {context:structuredClone(ctx)} : {})});
  const child=spawn("codex",["app-server"],{cwd,env:environment,stdio:["pipe","pipe","pipe"],detached:process.platform !== "win32"});
  let serial=0,stderr="";
  const pending=new Map(),observed=[],waiters=[];
  let failure;
  const deadline=setTimeout(()=>{
    failure=new Error(`Capture ${scenario} deadline exceeded`);
    for(const p of pending.values())p.reject(failure);
    for(const w of waiters)w.reject(failure);
    child.kill();
  },90000);
  child.stderr.on("data",chunk=>{stderr=(stderr+chunk).slice(-1000);});
  child.on("error",error=>{failure=error;for(const p of pending.values())p.reject(error);for(const w of waiters)w.reject(error);});
  child.on("exit",code=>{
    if(pending.size || waiters.length){failure=new Error(`CLI exited ${code}; ${stderr.slice(-300)}`);for(const p of pending.values())p.reject(failure);for(const w of waiters)w.reject(failure);}
  });
  createInterface({input:child.stdout}).on("line",line=>{
    let frame;try{frame=JSON.parse(line);}catch{return;}
    if(frame.method){
      if((frame.id!==undefined && requests.has(frame.method)) || (frame.id===undefined && notifications.has(frame.method))) {
        const decoded=frame.id===undefined ? {kind:"notification",method:frame.method,params:frame.params??{}} : {kind:"request",requestId:frame.id,method:frame.method,params:frame.params??{}};
        record({direction:"inbound",frame:decoded});
        if(frame.params?.item?.type === "userMessage") ctx.claimedSends=ctx.claimedSends.filter(claim=>claim.sendId !== frame.params.item.clientId);
      }
      observed.push(frame);
      for(let i=waiters.length-1;i>=0;i--)if(waiters[i].predicate(frame)){const w=waiters.splice(i,1)[0];w.resolve(frame);}
    }else if(pending.has(frame.id)){
      const p=pending.get(frame.id);pending.delete(frame.id);
      const result=p.method === "account/rateLimits/read" ? {rateLimits:frame.result?.rateLimits??null} : p.method === "account/read" ? {account:frame.result?.account ? {type:frame.result.account.type,planType:frame.result.account.planType} : null,requiresOpenaiAuth:frame.result?.requiresOpenaiAuth} : frame.result;
      const reply=frame.error ? {kind:"error",error:frame.error} : {kind:"success",value:result??null};
      record({direction:"inbound",operation:p.index,reply});
      frame.error ? p.reject(new Error(frame.error.message)) : p.resolve(frame.result);
    }
  });
  const wait = predicate => {
    const frame=observed.find(predicate);if(frame)return Promise.resolve(frame);
    if(failure)return Promise.reject(failure);
    return new Promise((resolve,reject)=>waiters.push({predicate,resolve,reject}));
  };
  function rpc(operation,method,params,index){
    if(failure)return Promise.reject(failure);
    if(index===undefined){index=fixture.operations.length;fixture.operations.push({operation,context:structuredClone(ctx)});}
    const id=++serial;
    record({direction:"outbound",operation:index,action:{kind:"request",call:{method,params,...(method === "turn/interrupt" ? {target:{threadId:params.threadId,turnId:params.turnId}} : {})}}});
    const promise=new Promise((resolve,reject)=>pending.set(id,{index,method,resolve,reject}));
    child.stdin.write(JSON.stringify({id,method,params})+"\n");return promise;
  }
  try {
    await rpc({kind:"initialize"},"initialize",{clientInfo:{name:"domios",version:"1.14.7"},capabilities:{experimentalApi:true,optOutNotificationMethods:[]}});
    record({direction:"outbound",operation:0,action:{kind:"notification",notification:{method:"initialized",params:{}}}});
    child.stdin.write(JSON.stringify({method:"initialized",params:{}})+"\n");
    record({direction:"inbound",operation:0,reply:{kind:"success",value:null}});
    const account=await rpc(undefined,"account/read",{refreshToken:false},0);
    if(["chatgpt","chatgptAuthTokens"].includes(account?.account?.type)) await rpc(undefined,"account/rateLimits/read",{},0);
    const instructions="Keep responses minimal. Do only the explicitly requested trivial capture exercise.";
    const opened=await rpc({kind:"openThread",instructions,settings},"thread/start",{cwd,model,threadSource:"user",config:{check_for_update_on_startup:false,project_doc_max_bytes:65536,model_reasoning_effort:"low"},approvalPolicy:background ? "never" : "untrusted",approvalsReviewer:"user",sandbox:background ? "danger-full-access" : "read-only",developerInstructions:instructions});
    ctx.threadId=opened.thread.id;
    await rpc(undefined,"thread/inject_items",{threadId:ctx.threadId,items:[{type:"message",role:"developer",content:[{type:"input_text",text:"Domios thread binding marker."}]}]},1);
    const submit=async(text,sendId)=>{
      ctx.claimedSends.push({sendId,threadId:ctx.threadId,input:{text,attachments:[]}});
      const operation={kind:"submitTurn",threadId:ctx.threadId,sendId,input:{text,attachments:[]},settings};
      const response=await rpc(operation,"turn/start",{threadId:ctx.threadId,clientUserMessageId:sendId,input:[{type:"text",text}],cwd,approvalPolicy:background ? "never" : "untrusted",approvalsReviewer:"user",sandboxPolicy:{type:background ? "dangerFullAccess" : "readOnly"},model,effort:"low"});
      return response.turn.id;
    };
    if(background) {
      const port=control.address().port;
      const turn=await submit(`Run exactly one command: node -e "const s=require('net').connect(${port},'127.0.0.1');s.on('close',()=>process.exit(0));s.on('error',()=>process.exit(1))". Use a short yield so it remains running in the background. Do not wait for it or kill it; the capture harness owns cleanup. Reply STARTED after the command yields.`,"capture-background");
      await wait(f=>f.method === "turn/completed" && f.params?.turn?.id === turn);
      const call=async(action)=>rpc({kind:"backgroundProcesses",threadId:ctx.threadId,action},`thread/backgroundTerminals/${action.kind}`,{threadId:ctx.threadId,...(action.kind === "list" ? {cursor:action.cursor,limit:256} : action.kind === "terminate" ? {processId:action.processId} : {})});
      const page=await call({kind:"list",cursor:null});
      if(!page.data?.length || page.nextCursor !== null)throw new Error("Capture must contain a running background process and a complete page");
      for(const entry of page.data)await call({kind:"terminate",processId:entry.processId});
      const final=await call({kind:"list",cursor:null});
      if(final.data.length || final.nextCursor !== null)throw new Error("Background processes remain after terminate");
      await call({kind:"clean"});
      const prior=ctx.threadId;ctx.threadId="00000000-0000-0000-0000-000000000000";
      await call({kind:"list",cursor:null}).catch(()=>{});ctx.threadId=prior;
    }else if(scenario === "streaming") {
      const turn=await submit("Write about 800 words of clear prose explaining how rain forms. Use no tools. Do not ask questions. Begin writing immediately.","capture-streaming");
      await wait(f=>f.method === "turn/completed" && f.params?.turn?.id === turn);
    }else if(scenario==="multi-turn"){
      const first=await submit("Reply with only ONE.","capture-send-1");await wait(f=>f.method==="turn/completed" && f.params?.turn?.id===first);
      const second=await submit("Reply with only TWO.","capture-send-2");await wait(f=>f.method==="turn/completed" && f.params?.turn?.id===second);
    }else if(scenario==="compaction"){
      const first=await submit("Reply with only ONE.","capture-compact-1");await wait(f=>f.method==="turn/completed" && f.params?.turn?.id===first);
      await rpc({kind:"control",threadId:ctx.threadId,action:"compact",value:null},"thread/compact/start",{threadId:ctx.threadId});
      await wait(f=>f.method==="item/completed" && f.params?.item?.type==="contextCompaction");
      await wait(f=>f.method==="turn/completed" && f.params?.turn?.id!==first);
    }else if(scenario==="interrupt"){
      const turn=await submit("Count upward from 1 to 500, each number on a new line.","capture-interrupt");
      await wait(f=>f.method==="turn/started" && f.params?.turn?.id===turn);
      ctx.activeTurnId=turn;
      ctx.activeTurns=[{threadId:ctx.threadId,turnId:turn}];
      await rpc({kind:"interrupt",threadId:ctx.threadId,activeTurnId:turn},"turn/interrupt",{threadId:ctx.threadId,turnId:turn});
      await wait(f=>f.method==="turn/completed" && f.params?.turn?.id===turn);
    }else{
      const turn=await submit("Run a shell command to write the word capture into capture-proof.txt. If permission is refused, reply DECLINED and stop. Do not substitute another action.","capture-approval");
      const request=await wait(f=>f.id!==undefined && requests.has(f.method));
      ctx.pendingRequest={requestId:request.id,method:request.method,params:request.params};
      let answer,result;
      if(request.method==="item/tool/requestUserInput"){
        const q=request.params.questions[0];answer={kind:"text",text:"Stop this exercise"};result={answers:{[q.id]:{answers:[answer.text]}}};
      }else{answer={kind:"choice",values:["decline"]};result={decision:"decline"};}
      const index=fixture.operations.length;fixture.operations.push({operation:{kind:"answer",requestId:request.id,answer},context:structuredClone(ctx)});
      record({direction:"outbound",operation:index,action:{kind:"reply",requestId:request.id,result:{kind:"success",value:result}}});
      child.stdin.write(JSON.stringify({id:request.id,result})+"\n");
      record({direction:"inbound",operation:index,reply:{kind:"success",value:null}});
      await wait(f=>f.method==="turn/completed" && f.params?.turn?.id===turn);
    }
    const sanitize=value=>typeof value === "string" ? [cwd,process.env.HOME,process.env.USERPROFILE].filter(Boolean).reduce((text,path)=>text.replaceAll(path,path === cwd ? "/work/capture" : "/home/capture"),value) : Array.isArray(value) ? value.map(sanitize) : value && typeof value === "object" ? Object.fromEntries(Object.entries(value).map(([key,value])=>[key,sanitize(value)])) : value;
    const content=JSON.stringify(sanitize(fixture),null,2);
    writeFileSync(resolve(destination,scenario+".json"),content+"\n");
    console.log(`CAPTURED ${scenario}: ${fixture.operations.length} operations, ${fixture.rpc.length} frames`);
  }catch(error){console.error(`CAPTURE FAILED ${scenario}: ${error.message}`);process.exitCode=1;}
  finally{
    if(background && ctx.threadId) {
      try {await rpc({kind:"backgroundProcesses",threadId:ctx.threadId,action:{kind:"clean"}},"thread/backgroundTerminals/clean",{threadId:ctx.threadId});} catch {}
    }
    if(control) {
      for(const socket of controls)socket.destroy();
      await new Promise(resolve=>control.close(resolve));
    }
    clearTimeout(deadline);
    if(process.platform === "win32" && child.pid) {try {execFileSync("taskkill",["/PID",String(child.pid),"/T","/F"],{stdio:"ignore",windowsHide:true});} catch {}}
    if(process.platform !== "win32" && child.pid) {try {process.kill(-child.pid,"SIGKILL");} catch {}}
    const exited = child.exitCode !== null || child.signalCode !== null ? Promise.resolve() : once(child,"exit");
    child.stdin.end();child.kill();await exited;rmSync(cwd,{recursive:true,force:true});
  }
}
for(const scenario of scenarios.length ? scenarios : ["multi-turn","interrupt","approval","streaming"])await capture(scenario);
