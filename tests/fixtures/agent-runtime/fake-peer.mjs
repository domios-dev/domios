import { createInterface } from "node:readline";
import { execFileSync } from "node:child_process";
import { closeSync, writeSync } from "node:fs";
const identityKeys = [
  "CODETERM_CLI_PATH", "CODETERM_TAB_ID", "CODETERM_SESSION_BINDING_NONCE",
  "CODETERM_SOCKET_PATH", "CODETERM_API_URL", "CODETERM_API_PORT", "CODETERM_API_TOKEN",
];
if (process.argv[2] === "--cli-probe") {
  writeSync(process.stdout.fd, JSON.stringify({
    artifact: process.argv[3],
    executable: process.argv[4],
    environment: Object.fromEntries(identityKeys.map(key => [key, process.env[key]])),
  }));
  process.exit(0);
}
function resolveCli(env, nested) {
  const windows = process.platform === "win32";
  const shell = windows ? `${process.env.SystemRoot}\\System32\\cmd.exe` : "/bin/sh";
  return ["domios", "codeterm"].map(alias => {
    const command = nested
      ? (windows ? `cmd.exe /d /s /c ${alias}` : `/bin/sh -c '${alias}'`)
      : alias;
    return JSON.parse(execFileSync(shell, windows ? ["/d", "/s", "/c", command] : ["-c", command], {
      env, encoding: "utf8", timeout: 5000,
    }));
  });
}
const input = createInterface({input:process.stdin});
input.on("line",line=>{
  const frame=JSON.parse(line);
  if(frame.method==="fixture/exit"){
    process.stderr.write("opaque fixture stderr",()=>process.exit(frame.params.code));
    return;
  }
  if(frame.method==="fixture/drop"){
    closeSync(process.stdout.fd);
    return;
  }
  if(frame.method==="fixture/settings"){
    process.stdout.write(JSON.stringify({id:frame.id,result:{}})+"\n",()=>{
      if(frame.params.value.model!=="refuse")process.stdout.write(JSON.stringify({method:"fixture/settings/updated",params:{threadId:frame.params.threadId,settings:frame.params.value}})+"\n");
    });
    return;
  }
  let result={method:frame.method,params:frame.params};
  if(frame.method==="thread/turns/list")result={data:[{id:"turn-one",status:"completed"},{id:"turn-two",status:"completed"}],nextCursor:null};
  if(frame.method==="thread/fork")result={thread:{id:"fork-thread"}};
  if(frame.method==="fixture/environment"){
    result={environment:JSON.parse(execFileSync(process.execPath,["-e","process.stdout.write(JSON.stringify(Object.fromEntries(JSON.parse(process.argv[1]).map(key=>[key,process.env[key]]))))",JSON.stringify(frame.params.keys)],{encoding:"utf8"}))};
  }
  if(frame.method==="fixture/cli-resolution"){
    const baseline = { ...process.env };
    for (const key of Object.keys(baseline)) {
      if (key.toUpperCase() === "PATH") delete baseline[key];
    }
    baseline.PATH = frame.params.inheritedPath;
    result={
      baseline:resolveCli(baseline,false),
      child:resolveCli(process.env,false),
      grandchild:resolveCli(process.env,true),
    };
  }
  process.stdout.write(JSON.stringify({id:frame.id,result})+"\n");
});
