import { createHash } from "node:crypto";
import { createInterface } from "node:readline";
import { execFile, execFileSync } from "node:child_process";
import { promisify } from "node:util";
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
    tokenDigest: createHash("sha256").update(process.env.CODETERM_API_TOKEN ?? "").digest("hex"),
  }));
  process.exit(0);
}
const executeFile = promisify(execFile);
async function resolveCli(env, nested) {
  const windows = process.platform === "win32";
  const shell = windows ? `${process.env.SystemRoot}\\System32\\cmd.exe` : "/bin/sh";
  const probes = [];
  for (const alias of ["domios", "codeterm"]) {
    const command = nested
      ? (windows ? `cmd.exe /d /s /c ${alias}` : `/bin/sh -c '${alias}'`)
      : alias;
    const { stdout } = await executeFile(shell, windows ? ["/d", "/s", "/c", command] : ["-c", command], {
      env, encoding: "utf8",
    });
    probes.push(JSON.parse(stdout));
  }
  return probes;
}
const input = createInterface({input:process.stdin});
input.on("line",async line=>{
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
    result=JSON.parse(execFileSync(process.execPath,["-e",`
      const environment=Object.fromEntries(JSON.parse(process.argv[1]).map(key=>[key,process.env[key]]));
      const tokenDigest=require("node:crypto").createHash("sha256").update(process.env.CODETERM_API_TOKEN ?? "").digest("hex");
      process.stdout.write(JSON.stringify({environment,tokenDigest}));
    `,JSON.stringify(frame.params.keys)],{encoding:"utf8"}));
  }
  if(frame.method==="fixture/cli-resolution"){
    const baseline = { ...process.env };
    for (const key of Object.keys(baseline)) {
      if (key.toUpperCase() === "PATH") delete baseline[key];
    }
    baseline.PATH = frame.params.inheritedPath;
    try {
      result={
        baseline:await resolveCli(baseline,false),
        child:await resolveCli(process.env,false),
        grandchild:await resolveCli(process.env,true),
      };
    } catch (error) {
      process.stdout.write(JSON.stringify({id:frame.id,error:{code:-32603,message:String(error),data:{stderr:error.stderr}}})+"\n");
      return;
    }
  }
  process.stdout.write(JSON.stringify({id:frame.id,result})+"\n");
});
