#!/usr/bin/env node
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {catalogueCommand,runCatalogue} from './ci-catalogue-command.mjs';
import {inferRunnerProfile,runnerProfile} from './ci-runner-profile.mjs';
import {sourceSha,reproLine} from './ci-repro.mjs';
import {bootstrapStep,createProfileEnvironment,workflowStep} from './ci-step-bootstrap.mjs';

export function parseStepArgs(args) {
  const options={};
  if(args[0]==='--list')return{list:true};
  const [step,...flags]=args;
  if(!/^[a-z][a-z0-9-]+$/.test(step||''))throw new Error('Usage: node scripts/ci-step.mjs <catalogue-step> [--as profile] [--file path] [--checkout path] [--source-sha sha]');
  for(let index=0;index<flags.length;index++){
    const flag=flags[index];
    if(flag==='--ci'){options.ci=true;continue;}
    const key={'--as':'profile','--file':'file','--checkout':'checkout','--source-sha':'sha','--plugin-id':'pluginId'}[flag];
    if(!key||!flags[index+1]||flags[index+1].startsWith('--'))throw new Error('Invalid ci-step option: '+flag);
    options[key]=flags[++index];
  }
  if(options.profile)runnerProfile(options.profile);
  return{step,...options};
}
export function catalogueEntry(root,id) {
  const catalogue=JSON.parse(fs.readFileSync(path.join(root,'tests/catalogue.json'),'utf8'));
  const entry=catalogue.runners.find(row=>row.id===id);
  if(!entry||entry.status==='suspended')throw new Error('CI catalogue entry unavailable: '+id);
  if(entry.needs_live_agent||entry.needs_instance)throw new Error('ci-step only runs offline catalogue entries: '+id);
  return entry;
}
export function selectStep(root,entry,env,execute=spawnSync) {
  const selection=path.join(env.RUNNER_TEMP,'pr-catalogue.json');
  let selected;
  try{selected=JSON.parse(fs.readFileSync(selection,'utf8'));}catch{}
  if(!selected?.runners?.some(row=>row.id===entry.id)){
    const result=execute(process.execPath,['scripts/test-catalogue.mjs','select','--tier',entry.tier,'--format','json'],{cwd:root,env,encoding:'utf8',timeout:30000,windowsHide:true,maxBuffer:16*1024*1024});
    if(result.error||result.status!==0)throw new Error('CI catalogue selection failed: '+(result.stderr||result.error?.message||''));
    fs.writeFileSync(selection,result.stdout);
  }
}
export async function runStep(options,{base=process.env,execute=spawnSync,write=console.error}={}) {
  const root=path.resolve(options.checkout||process.cwd());
  const profile=options.profile||inferRunnerProfile(base);
  const sha=sourceSha(root,base,execute);
  let temporary;
  try{
    if(options.sha&&options.sha!==sha)throw new Error('Source SHA differs: checkout '+options.sha+' before reproducing (actual '+sha+')');
    const entry=catalogueEntry(root,options.step);
    let env={...base,CI:'true',CI_RUNNER_PROFILE:profile,CI_SOURCE_SHA:sha,CI_STEP:options.step};
    if(options.profile&&!options.ci){
      temporary=fs.mkdtempSync(path.join(os.tmpdir(),'domios-ci-step-'));
      env=createProfileEnvironment(profile,env,temporary);
    }else if(!env.RUNNER_TEMP){
      temporary=fs.mkdtempSync(path.join(os.tmpdir(),'domios-ci-step-'));
      env.RUNNER_TEMP=temporary;env.GITHUB_ENV=path.join(temporary,'github-env');env.GITHUB_PATH=path.join(temporary,'github-path');
      fs.writeFileSync(env.GITHUB_ENV,'');fs.writeFileSync(env.GITHUB_PATH,'');
    }
    const owner=workflowStep(root,entry.id,profile);
    bootstrapStep(owner,{root,env,profile,execute,ci:options.ci});
    selectStep(root,entry,env,execute);
    let status;
    if(options.file){
      const discovered=execute(process.execPath,['scripts/test-catalogue.mjs','list','--entry',entry.id],{cwd:root,env,encoding:'utf8',timeout:30000,windowsHide:true});
      if(discovered.status!==0||!discovered.stdout.trim().split(/\r?\n/).includes(options.file))throw new Error('Focused file is not owned by '+entry.id+': '+options.file);
      if(!entry.exec)throw new Error('--file requires a catalogue entry with discovered executable tests');
      const {runCommands}=await import(pathToFileURL(path.join(root,'scripts/test-catalogue.mjs')).href);
      const overlay=JSON.parse(fs.readFileSync(path.join(root,'tests/catalogue.overlay.json'),'utf8'));
      status=0;
      for(const argv of runCommands(entry,[options.file],overlay,{python:env.CI_PYTHON||'python3',node:process.execPath})){
        const result=execute(argv[0],argv.slice(1),{cwd:root,env,stdio:'inherit',windowsHide:true});
        if(result.error)throw result.error;
        if(result.status!==0)status=result.status||1;
      }
    }else{
      const command=catalogueCommand(entry.id,{directory:env.RUNNER_TEMP,pluginId:options.pluginId});
      console.log('CI step '+entry.id+': '+command);
      status=runCatalogue(entry.id,{env,pluginId:options.pluginId,execute:(program,args,settings)=>execute(program,args,{...settings,cwd:root})});
    }
    if(status!==0)write(reproLine(entry.id,{...options,profile,sha}));
    return status;
  }catch(error){
    write('CI step failed: '+error.message);
    write(reproLine(options.step,{...options,profile,sha}));
    return error.exitCode||1;
  }finally{
    if(temporary)fs.rmSync(temporary,{recursive:true,force:true});
  }
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
  try{
    const options=parseStepArgs(process.argv.slice(2));
    if(options.list){
      const catalogue=JSON.parse(fs.readFileSync('tests/catalogue.json','utf8'));
      console.log(catalogue.runners.filter(row=>!row.needs_instance&&!row.needs_live_agent&&row.status!=='suspended').map(row=>row.id).sort().join('\n'));
    }else process.exitCode=await runStep(options);
  }catch(error){console.error(error.message);process.exitCode=2;}
}
