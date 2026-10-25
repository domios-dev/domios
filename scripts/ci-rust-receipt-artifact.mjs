import fs from 'node:fs';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {bashInterpreter} from './ci-runner-paths.mjs';

export function receiptArtifactKey(file,env=process.env){
  if(!/^[1-9]\d*$/.test(env.GITHUB_RUN_ID||'')||!/^[1-9]\d*$/.test(env.GITHUB_RUN_ATTEMPT||''))throw new Error('Receipt artifact requires run and attempt identity');
  const producer=[env.GITHUB_JOB,env.RUNNER_OS,env.RUNNER_ARCH].join('-');
  if(!/^[A-Za-z0-9_.-]+$/.test(producer)||!env.GITHUB_JOB||!env.RUNNER_OS||!env.RUNNER_ARCH)throw new Error('Receipt artifact requires job/OS/architecture identity');
  if(!/^[A-Za-z0-9_.-]+\.(json|log)$/.test(file))throw new Error('Invalid receipt artifact filename');
  return env.GITHUB_RUN_ID+'/rust-receipt/'+producer+'-attempt-'+env.GITHUB_RUN_ATTEMPT+'-'+file;
}
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
  const directory='ci-rust-receipts';
  const files=fs.readdirSync(directory).filter(file=>/\.(json|log)$/.test(file));
  if(!files.length)throw new Error('No Rust test receipts to store');
  for(const file of files)execFileSync(bashInterpreter(),['scripts/ci-artifact.sh','put',receiptArtifactKey(file),directory+'/'+file],{stdio:'inherit',timeout:300000});
}
