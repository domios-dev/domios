import fs from 'node:fs';
import {reproLine} from './ci-repro.mjs';
import { pythonInterpreter } from './ci-runner-paths.mjs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

export function preparePython({env=process.env,platform=process.platform,execute=spawnSync,append=fs.appendFileSync}={}){
  if(!env.RUNNER_TEMP||!env.GITHUB_PATH||!env.GITHUB_ENV)throw new Error('Python bootstrap requires job-local runner paths');
  const base=pythonInterpreter(platform,env);
  const directory=path.join(env.RUNNER_TEMP,'.ci-venv');
  const bin=path.join(directory,platform==='win32'?'Scripts':'bin');
  const interpreter=path.join(bin,platform==='win32'?'python.exe':'python3');
  const run=(program,args)=>{
    const result=execute(program,args,{env,stdio:'inherit',windowsHide:true});
    if(result.error||result.status!==0)throw new Error('Job-local Python bootstrap failed: '+program+' '+args.join(' ')+'. Restore Python and its venv module using .agents/skills/build-machine/SKILL.md');
  };
  run(base,['-m','venv',directory]);
  run(interpreter,['-m','pip','install','--disable-pip-version-check','-r',fileURLToPath(new URL('./requirements-ci.txt',import.meta.url))]);
  append(env.GITHUB_PATH,bin+'\n');
  append(env.GITHUB_ENV,'CI_PYTHON='+interpreter+'\nVIRTUAL_ENV='+directory+'\n');
  return interpreter;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
  try{preparePython();}catch(error){
    console.error(error.stack||error.message);
    console.error(reproLine(process.env.CI_STEP||'python-script-tests'));
    process.exitCode=1;
  }
}
