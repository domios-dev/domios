import {spawn} from "node:child_process";
import {writeFileSync} from "node:fs";
import {createInterface} from "node:readline";
import {fileURLToPath} from "node:url";
const mode=process.argv[2];
if (mode==="descendant") {
  process.stdin.resume();
} else {
  const child=spawn(process.execPath,[fileURLToPath(import.meta.url),"descendant"],{stdio:["pipe","inherit","inherit"]});
  const spawned=new Promise((resolve,reject)=>{child.once("spawn",resolve);child.once("error",reject);});
  const lines=createInterface({input:process.stdin});
  lines.once("line",async(line)=>{
    await spawned;
    const request=JSON.parse(line);
    const safe=Object.keys(process.env).every(key=>!key.toUpperCase().startsWith("CODETERM_") && key.toUpperCase()!=="FORCE_CODE_TERMINAL")
      && process.env.ENABLE_CLAUDEAI_MCP_SERVERS==="false" && process.env.CLAUDE_CODE_AUTO_CONNECT_IDE==="0" && process.env.CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL==="1";
    writeFileSync("receipt.json",JSON.stringify({request,environment_safe:safe,args:process.argv.slice(3),pids:[process.pid,child.pid]}));
    process.stdout.write(JSON.stringify({type:"control_response",response:{request_id:"wrong",subtype:"success",response:{commands:[{name:"wrong"}]}}})+"\n");
    if (mode==="success") process.stdout.write(JSON.stringify({type:"control_response",response:{request_id:request.request_id,subtype:"success",response:{commands:[{name:"fixture"}]}}})+"\n");
  });
}
