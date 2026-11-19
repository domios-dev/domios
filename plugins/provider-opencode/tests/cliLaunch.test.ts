import assert from "node:assert/strict";
import {test} from "node:test";
import fs from "node:fs";
import vm from "node:vm";
import {generation,v2Arguments} from "../src/cliLaunch";
import {ioTestContext} from "../../provider-shared/tests/hostIo";
import {ocHost} from "./hostFixture.testdata";

const fixture=JSON.parse(fs.readFileSync(new URL("./fixtures/cli-launch-contract.json",import.meta.url),"utf8"));
const source=fs.readFileSync(new URL("../plugin.js",import.meta.url),"utf8");
function plugin(platform:string,version:string|null="2.3.0",ports:Record<string,unknown>={}) {
  const context={host:{...ocHost({platform:()=>platform,providerVersionJson:()=>JSON.stringify({version,status:version?"detected":"pending"}),
    envGet:(name:string)=>name==="CODETERM_PAD_DIR"?"C:/opaque/pad":null,
    shell:{quoteFor:(value:string)=>`'${value.replace(/'/g,"''")}'`},
    fs:{fileExists:()=>false,writeTempFile:()=>"C:/opaque/instructions.md"}}),...ports},module:{exports:{} as {default?:any}},exports:{}};
  new vm.Script(source).runInNewContext(ioTestContext(context));
  return context.module.exports.default;
}

test("captured CLI versions select only documented launch contracts",()=>{
  for (const entry of fixture.cases) assert.equal(generation(entry.version)??null,entry.generation);
});
test("v2 root model agent and variant become one process-local config",()=>{
  assert.deepEqual(v2Arguments(fixture.modern.args),{args:[],config:fixture.modern.config});
  assert.deepEqual(v2Arguments(["--model=vendor/model#max","--variant=default"]),{args:[],config:{model:"vendor/model"}});
  assert.throws(()=>v2Arguments(["--model","unqualified"]),/qualified/);
  assert.throws(()=>v2Arguments(["--variant","high"]),/explicit model/);
  assert.throws(()=>v2Arguments(["--server","http://foreign"]),/ownership/);
});
test("v2 fresh restore and contextual resume use private servers without unsupported root flags",()=>{
  for(const platform of ["windows","linux"]){
    const p=plugin(platform);
    const commands=[p.buildLaunchCommand({task:"opaque",args:fixture.modern.args,skipPermissions:true,structuredPromptServer:{hostname:"127.0.0.1",port:53626}}),
      p.buildResumeCommand(fixture.modern.session,true),
      p.buildResumeCommandWithContext({sessionId:fixture.modern.session,args:fixture.modern.args,systemPrompt:"opaque"})];
    for(const command of commands){
      assert.match(command,/opencode --standalone/);
      assert.doesNotMatch(command,/--hostname|--port|'-m'|'--model'|'--agent'|'--variant'/);
      assert.ok(command.includes("OPENCODE_CONFIG_CONTENT"));
      assert.ok(command.includes("CODETERM_RESUME_SESSION_ID"));
    }
    assert.ok(commands[0].includes('"model":"alibaba-coding-plan/glm-5#high"'));
    assert.ok(commands[0].includes('"instructions":["C:/opaque/instructions.md"]'));
    assert.ok(commands[1].includes(`--session '${fixture.modern.session}'`));
  }
});
test("v2 permissions preserve unmatched ask and host-owned tool denials after the pad grant",()=>{
  const command=plugin("windows").buildLaunchCommand({hostOwned:{questions:true,todos:true}});
  const raw=command.match(/\$env:OPENCODE_CONFIG_CONTENT = '([^']*)'/)?.[1];
  assert.ok(raw);
  const config=JSON.parse(raw);
  assert.deepEqual(config.permissions,[{action:"*",resource:"*",effect:"ask"},{action:"*",resource:"C:/opaque/pad/*",effect:"allow"},
    ...["question","todoread","todowrite"].map(action=>({action,resource:"*",effect:"deny"}))]);
  assert.ok(command.includes("$env:OPENCODE_CONFIG_CONTENT = $ct_previous_config"));
});
test("unknown contracts fail before constructing an agent command",()=>{
  assert.throws(()=>plugin("windows","3.0.0").buildLaunchCommand({}),/Unsupported OpenCode CLI version/);
  assert.throws(()=>plugin("windows",null).buildLaunchCommand({}),/capabilities could not be read/);
});
test("v2 catalogue discovery uses a private server and never sends the removed verbose flag",()=>{
  const calls:string[][]=[];
  const p=plugin("windows","2.3.0",{
    execStart:(raw:string)=>{calls.push(JSON.parse(raw).args);return JSON.stringify({jobId:"catalogue"});},
    awaitJob:(_id:string,then:(reply:unknown)=>unknown)=>then({code:0,stdout:"opencode/big-pickle\n"}),
  });
  p.discoverModels();
  assert.deepEqual(calls,[["models","--standalone"]]);
});
