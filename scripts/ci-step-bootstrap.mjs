import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createRequire} from 'node:module';
import {createHash} from 'node:crypto';
import {checkRunnerTools,pythonInterpreter,runnerToolPaths,runnerRequirements} from './ci-runner-paths.mjs';
import {runnerProfile,profileEnvironment} from './ci-runner-profile.mjs';

const files=directory=>{try{return fs.readdirSync(directory);}catch{return[];}};
export function discoveredPaths(env=process.env,platform=process.platform) {
  const has=file=>fs.existsSync(file);
  const inherited=(env.PATH||'').split(path.delimiter);
  const programs=['node','git','bash','cargo','rustup','cargo-nextest','pnpm','jq','curl','tar'];
  return [...new Set([...runnerToolPaths(platform,env,has,files),
    ...inherited.filter(directory=>programs.some(program=>has(path.join(directory,program+(platform==='win32'?'.exe':'')))||
      platform==='win32'&&has(path.join(directory,program+'.cmd')))),env.PNPM_HOME].filter(Boolean))];
}
export function createProfileEnvironment(name,base,directory) {
  const env=profileEnvironment(name,base,directory,{toolPaths:discoveredPaths(base)});
  for(const key of Object.keys(env))if(/(?:TOKEN|PASSWORD|SECRET|PRIVATE_KEY|CERTIFICATE)$/.test(key)||key.startsWith('CODETERM_API_'))delete env[key];
  fs.mkdirSync(env.CARGO_HOME,{recursive:true});
  const bin=path.join(directory,'profile-bin');fs.mkdirSync(bin,{recursive:true});
  const python=pythonInterpreter(process.platform,base);
  env.CI_PYTHON=python;
  const command=runnerProfile(name).python;
  const quoted="'"+python.replaceAll("'", "'\\''").replaceAll('\\','/')+"'";
  fs.writeFileSync(path.join(bin,command),'#!/usr/bin/env bash\nexec '+quoted+' "$@"\n',{mode:0o755});
  if(process.platform==='win32')fs.writeFileSync(path.join(bin,command+'.cmd'),'@"'+python+'" %*\r\n');
  if(command==='python3'){
    fs.writeFileSync(path.join(bin,'python'),'#!/usr/bin/env bash\necho "python: command not found (profile exposes python3)" >&2\nexit 127\n',{mode:0o755});
    if(process.platform==='win32')fs.writeFileSync(path.join(bin,'python.cmd'),'@echo python: command not found (profile exposes python3) 1>&2\r\n@exit /b 127\r\n');
  }
  env.PATH=bin+path.delimiter+env.PATH;
  for(const file of [env.GITHUB_ENV,env.GITHUB_PATH])fs.writeFileSync(file,'');
  return env;
}
export function workflowStep(root,id,profile) {
  const manifest=path.join(root,'scripts/ci-step-contracts.json');
  const preferred=runnerProfile(profile).environment==='github-hosted'?'tests-pr.yml':'tests-fast.yml';
  if(fs.existsSync(manifest)){
    const data=JSON.parse(fs.readFileSync(manifest,'utf8'));
    const candidates=data.bindings.filter(row=>row.entry===id).sort((a,b)=>Number(b.file===preferred)-Number(a.file===preferred));
    const owner=candidates[0];
    if(!owner)return null;
    const text=fs.readFileSync(path.join(root,'.github/workflows',owner.file),'utf8').replaceAll('\r\n','\n');
    if(createHash('sha256').update(text).digest('hex')!==data.sources[owner.file])throw new Error('CI step contracts stale: run node scripts/generate-ci-step-contracts.mjs');
    return owner;
  }
  // Historical checkouts predate the generated bindings; the current harness supplies its parser.
  const {parse}=createRequire(import.meta.url)('yaml');
  const directory=path.join(root,'.github/workflows');
  const names=files(directory).filter(file=>file.endsWith('.yml')).sort((a,b)=>Number(b===preferred)-Number(a===preferred));
  for(const file of names){
    const graph=parse(fs.readFileSync(path.join(directory,file),'utf8'));
    for(const[jobId,job]of Object.entries(graph.jobs||{})){
      const index=(job.steps||[]).findIndex(step=>new RegExp('(?:ci-step|ci-catalogue-command)\\.mjs\\s+'+id+'(?:\\s|$)|test-catalogue\\.mjs\\s+run\\s+--entry\\s+'+id+'(?:\\s|$)').test(step.run||''));
      if(index<0)continue;
      const before=job.steps.slice(0,index);
      const preflight=before.find(row=>/ci-runner-paths\.mjs\s+(?!--paths-only)/.test(row.run||''));
      return{file,jobId,entry:id,role:preflight?.run.trim().split(/\s+/).at(-1)||'node,git,bash',
        pythonBootstrap:before.find(row=>/ci-python-env\.mjs|python3?\s+-m\s+pip\s+install/.test(row.run||''))?.run||null};
    }
  }
  return null;
}
export function applyJobFiles(env) {
  if(fs.existsSync(env.GITHUB_ENV))for(const line of fs.readFileSync(env.GITHUB_ENV,'utf8').split(/\r?\n/)){
    const at=line.indexOf('=');if(at>0)env[line.slice(0,at)]=line.slice(at+1);
  }
  if(fs.existsSync(env.GITHUB_PATH)){
    const additions=fs.readFileSync(env.GITHUB_PATH,'utf8').split(/\r?\n/).filter(Boolean).reverse();
    env.PATH=[...additions,env.PATH].join(path.delimiter);
  }
}
export function bootstrapStep(owner,{root,env,profile,execute=spawnSync,ci=false}){
  const run=(program,args,timeout=120000)=>{
    const result=execute(program,args,{cwd:root,env,stdio:'inherit',timeout,windowsHide:true});
    if(result.error||result.status!==0)throw Object.assign(new Error('CI bootstrap failed: '+program+' '+args.join(' ')),{exitCode:result.status||1,cause:result.error});
  };
  const python=owner?.pythonBootstrap;
  if(python&&!(env.VIRTUAL_ENV&&env.CI_PYTHON&&fs.existsSync(env.CI_PYTHON))){
    if(python==='node scripts/ci-python-env.mjs')run(process.execPath,['scripts/ci-python-env.mjs']);
    else run('bash',['-euo','pipefail','-c',python]);
    applyJobFiles(env);
  }
  const role=owner?.role||'node,git,bash';
  if(runnerRequirements(role).includes('cargo')){
    const source=fs.readFileSync(path.join(root,'rust-toolchain.toml'),'utf8');
    const channel=/channel\s*=\s*"([^"]+)"/.exec(source)?.[1];
    if(!channel)throw new Error('Pinned Rust channel is unavailable');
    env.RUSTUP_TOOLCHAIN=channel;
    const probe=execute('rustup',['run',channel,'cargo','--version'],{cwd:root,env,encoding:'utf8',timeout:2000,windowsHide:true});
    if(probe.error||probe.status!==0){
      if(runnerProfile(profile).environment!=='github-hosted'||ci)throw new Error('Pinned Rust toolchain unavailable: '+channel+'\n'+(probe.stderr||probe.error?.message||''));
      run('rustup',['toolchain','install',channel,'--profile','minimal','--component','clippy,rustfmt']);
    }
  }
  checkRunnerTools(role,{env,execute});
}
