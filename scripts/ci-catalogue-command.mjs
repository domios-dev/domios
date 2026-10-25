import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export function catalogueCommand(id,{directory=process.env.RUNNER_TEMP,pluginId,read=fs.readFileSync}={}){
  if(!directory)throw new Error('CI catalogue unavailable: RUNNER_TEMP is missing; run Select PR catalogue first');
  let selection;
  try { selection=JSON.parse(read(path.join(directory,'pr-catalogue.json'),'utf8')); }
  catch(error){throw new Error('CI catalogue unavailable: missing or invalid selection for '+id+'; run Select PR catalogue first ('+error.message+')');}
  let command=selection.runners?.find(entry=>entry.id===id)?.runner;
  if(typeof command!=='string'||!command.trim())throw new Error('CI catalogue entry unavailable: '+id+'; run Select PR catalogue with the required tier');
  if(command.includes('<plugin-id>')){
    if(!/^[a-zA-Z0-9_.-]+$/.test(pluginId||''))throw new Error('CI catalogue plugin id is missing or invalid');
    command=command.replaceAll('<plugin-id>',pluginId);
  }
  return command;
}
export function runCatalogue(id,{env=process.env,pluginId,execute=spawnSync,...options}={}){
  const command=catalogueCommand(id,{directory:env.RUNNER_TEMP,pluginId,...options});
  const result=execute('bash',['-c',command],{env,stdio:'inherit',windowsHide:true});
  if(result.error)throw result.error;
  return result.status??1;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
  try {process.exitCode=runCatalogue(process.argv[2],{pluginId:process.argv[3]==='--plugin-id'?process.argv[4]:undefined});}
  catch(error){console.error('::error title=CI catalogue unavailable::'+error.message);process.exitCode=1;}
}
