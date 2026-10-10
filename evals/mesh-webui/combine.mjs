import {readFile,writeFile,mkdir} from 'node:fs/promises';
import path from 'node:path';
import {providers,directions,matrix,validateOutput} from './model.mjs';
const run=process.argv[2];
if(!/^[a-zA-Z0-9_-]+$/.test(run)) throw new Error('Invalid run name');
const rc='/Users/kandrey/Developer/codeterm-worktrees/rc-mac/target-check/evals';
const out=validateOutput(path.join(rc,'mesh-webui-'+run));
await mkdir(out,{recursive:false});
const rows=[],errors=[];
for(const direction of directions) for(const provider of providers) {
  const name=`mesh-webui-${run}-${direction}-${provider}`;
  try {
    const receipts=JSON.parse(await readFile(path.join(rc,name,'receipts.json'),'utf8'));
    rows.push(...receipts.map(r=>({...r,evidence:`../${name}/${r.evidence}`,screenshot:r.screenshot?`../${name}/${r.screenshot}`:null})));
    try {errors.push({case:name,failure:JSON.parse(await readFile(path.join(rc,name,'run-failure.json'),'utf8'))});} catch(error) {if(error.code!=='ENOENT') throw error;}
  } catch(error) {errors.push({case:name,error:String(error)});}
}
await writeFile(path.join(out,'results.md'),matrix(rows));
await writeFile(path.join(out,'receipts.json'),JSON.stringify(rows,null,2));
await writeFile(path.join(out,'case-failures.json'),JSON.stringify(errors,null,2));
if(errors.length||rows.some(r=>r.status==='FAIL')) process.exitCode=1;
