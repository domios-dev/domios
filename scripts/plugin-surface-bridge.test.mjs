import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import assert from "node:assert/strict";
import test from "node:test";

test("materialized surface bridge accepts only host invalidations and unsubscribes", () => {
  const source = readFileSync(new URL("../src-tauri/crates/codeterm-api/src/web_plugin_routes.rs", import.meta.url), "utf8");
  const script = source.match(/const PLUGIN_BRIDGE_JS: &str = r#"([\s\S]*?)"#;/)[1]
    .replace("__CT_SURFACE__", JSON.stringify({surface:"board",instance:"one",tab:"tab"}));
  const listeners = [];
  const receive = (event) => listeners.forEach((listener) => listener(event));
  const messages = [];
  const parent = {postMessage: message => messages.push(message)};
  const sandbox = {parent, document:{body:null, documentElement:{style:{}},readyState:"loading",addEventListener(){}},
    addEventListener: (event, callback) => { assert.equal(event,"message"); listeners.push(callback); }};
  sandbox.window = sandbox;
  runInNewContext(script, sandbox);
  let count = 0;
  const release = sandbox.ct.onSurfaceStateChanged(() => count++);
  receive({source:{},data:{__ctSurfaceStateChanged:true}});
  receive({source:parent,data:{__ctSurfaceStateChanged:"arbitrary"}});
  assert.equal(count,0);
  receive({source:parent,data:{__ctSurfaceStateChanged:true}});
  assert.equal(count,1);
  release();
  release();
  receive({source:parent,data:{__ctSurfaceStateChanged:true}});
  assert.equal(count,1);
  assert.equal(messages[0].__ctReady,true);
});
