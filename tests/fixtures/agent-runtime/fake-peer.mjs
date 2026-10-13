import { createInterface } from "node:readline";
import { execFileSync } from "node:child_process";
const input = createInterface({input:process.stdin});
input.on("line",line=>{
  const frame=JSON.parse(line);
  if(frame.method==="fixture/exit"){
    process.stderr.write("opaque fixture stderr",()=>process.exit(frame.params.code));
    return;
  }
  if(frame.method==="fixture/drop"){
    process.stdout.end();
    return;
  }
  let result={method:frame.method,params:frame.params};
  if(frame.method==="fixture/environment"){
    result={environment:JSON.parse(execFileSync(process.execPath,["-e","process.stdout.write(JSON.stringify(Object.fromEntries(JSON.parse(process.argv[1]).map(key=>[key,process.env[key]]))))",JSON.stringify(frame.params.keys)],{encoding:"utf8"}))};
  }
  process.stdout.write(JSON.stringify({id:frame.id,result})+"\n");
});
