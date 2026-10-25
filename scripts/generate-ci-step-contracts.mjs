#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {parse} from 'yaml';
import {fileURLToPath} from 'node:url';

export function stepBindings(root=process.cwd()){
  const directory=path.join(root,'.github/workflows'),bindings=[],sources={};
  for(const file of fs.readdirSync(directory).filter(file=>file.endsWith('.yml'))){
    const text=fs.readFileSync(path.join(directory,file),'utf8');
    let used=false;
    for(const[jobId,job]of Object.entries(parse(text).jobs||{})){
      const steps=job.steps||[];
      for(const[index,step]of steps.entries()){
        const pattern=/(?:ci-step|ci-catalogue-command)\.mjs\s+([\w-]+)|test-catalogue\.mjs\s+run\s+--entry\s+([\w-]+)/g;
        for(const match of (step.run||'').matchAll(pattern)){
          const before=steps.slice(0,index);
          const preflight=before.find(row=>/ci-runner-paths\.mjs\s+(?!--paths-only)/.test(row.run||''));
          bindings.push({file,jobId,entry:match[1]||match[2],
            role:preflight?.run.trim().split(/\s+/).at(-1)||'node,git,bash',
            pythonBootstrap:before.find(row=>/ci-python-env\.mjs|python3?\s+-m\s+pip\s+install/.test(row.run||''))?.run||null});
          used=true;
        }
      }
    }
    if(used)sources[file]=createHash('sha256').update(text.replaceAll('\r\n','\n')).digest('hex');
  }
  return{version:1,sources,bindings};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  const output=JSON.stringify(stepBindings(),null,2)+'\n';
  const file='scripts/ci-step-contracts.json';
  if(process.argv.includes('--check')){
    if(fs.readFileSync(file,'utf8').replaceAll('\r\n','\n')!==output)throw new Error('CI step contracts stale: run node scripts/generate-ci-step-contracts.mjs');
  }else fs.writeFileSync(file,output);
}
