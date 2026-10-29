import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import assert from "node:assert/strict";
import test from "node:test";

function fixture() {
  const source=readFileSync(new URL("../src-tauri/crates/codeterm-api/src/web_plugin_routes.rs",import.meta.url),"utf8");
  const script=source.match(/const PLUGIN_BRIDGE_JS: &str = r#"([\s\S]*?)"#;/)[1]
    .replace("__CT_SURFACE__",JSON.stringify({surface:"board",instance:"one",tab:"tab"}))
    .replace("__CT_DOCUMENT__",JSON.stringify("a".repeat(32)));
  const messages=[];const posted=[];const listeners=[];let ports;
  class Port {
    postMessage(message){posted.push(message);}
    start(){}
  }
  class MessageChannel {
    constructor(){this.port1=new Port();this.port2=new Port();ports=this;}
  }
  const parent={postMessage:(message,target,transfer)=>messages.push({message,target,transfer})};
  const sandbox={parent,MessageChannel,Event:class{constructor(type){this.type=type;}},
    MessageEvent:class{constructor(type,value){this.type=type;Object.assign(this,value);}},
    document:{body:null,documentElement:{style:{}},readyState:"loading",addEventListener(){}},
    addEventListener:(event,callback)=>listeners.push({event,callback}),dispatchEvent(){} };
  sandbox.window=sandbox;runInNewContext(script,sandbox);
  return {sandbox,messages,posted,ports,Port,listeners,receive:data=>ports.port2.onmessage({data})};
}

test("surface invalidation and host support arrive only on the transferred private port",async()=>{
  const f=fixture();let count=0;const release=f.sandbox.ct.onSurfaceStateChanged(()=>count++);
  // No public-window receive callback exists for authority or replies.
  assert.equal(f.listeners.filter(item=>item.event==="message").length,0);
  assert.equal(f.messages.length,1);
  assert.equal(f.messages[0].message.__ctReady,"a".repeat(32));
  assert.equal(f.messages[0].transfer[0],f.ports.port1);
  f.receive({__ctSurfaceStateChanged:"arbitrary"});assert.equal(count,0);
  f.receive({__ctSurfaceStateChanged:true});assert.equal(count,1);
  release();release();f.receive({__ctSurfaceStateChanged:true});assert.equal(count,1);
  const available=f.sandbox.ct.host.availability();
  f.receive({__ctHostSupport:{copyText:true,saveAs:true,reveal:false}});
  assert.equal((await available).copyText,true);
});

test("initial invoke, host gestures, media, close, UI state and replies use only the document port",async()=>{
  const f=fixture();f.receive({__ctChallenge:7});
  assert.equal(f.posted.pop().__ctProof,7);
  const invoke=f.sandbox.ct.invoke("read",{opaque:true});
  const request=f.posted.pop();assert.equal(request.method,"read");
  f.receive({__ctReply:request.__ctInvoke,result:"ok"});assert.equal(await invoke,"ok");
  const copy=f.sandbox.ct.host.copyText("opaque");const action=f.posted.pop();assert.equal(action.action,"copyText");
  f.receive({__ctReply:action.__ctHost,result:{ok:true}});assert.equal((await copy).ok,true);
  const media=f.sandbox.ct.media("output");const lookup=f.posted.pop();assert.equal(lookup.id,"output");
  f.receive({__ctReply:lookup.__ctMedia,result:{handle:"opaque"}});assert.equal((await media).handle,"opaque");
  f.sandbox.ct.__reportUiState({status:"ready"});assert.equal(f.posted.pop().__ctUiState.status,"ready");
  f.sandbox.ct.close();assert.equal(f.posted.pop().__ctClose,true);
  assert.equal(f.messages.length,1);
});

test("plugin prototype changes cannot capture or replace the bootstrap's private sending endpoint",()=>{
  const f=fixture();let stolen=false;
  f.Port.prototype.postMessage=function(){stolen=true;};
  f.receive({__ctChallenge:9});
  assert.equal(stolen,false);assert.equal(f.posted.pop().__ctProof,9);
});
