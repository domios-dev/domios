import { createInterface } from "node:readline";
import { execFileSync } from "node:child_process";
import { closeSync } from "node:fs";
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
  process.stdout.write(JSON.stringify({id:frame.id,result})+"\n");
});
