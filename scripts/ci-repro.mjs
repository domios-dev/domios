import {spawnSync} from 'node:child_process';
import {inferRunnerProfile} from './ci-runner-profile.mjs';

export function sourceSha(cwd=process.cwd(),env=process.env,execute=spawnSync) {
  const result=execute('git',['rev-parse','HEAD'],{cwd,env,encoding:'utf8',timeout:2000,windowsHide:true});
  const sha=result.status===0?result.stdout.trim():env.CI_SOURCE_SHA||env.GITHUB_SHA;
  return /^[a-f0-9]{40}$/.test(sha||'')?sha:'unavailable';
}
export function reproLine(step,{profile=inferRunnerProfile(),sha=sourceSha(),checkout,file,pluginId}={}) {
  const argument=value=>/^[A-Za-z0-9_./:-]+$/.test(value)?value:JSON.stringify(value);
  return 'repro: node scripts/ci-step.mjs '+argument(step)+' --as '+argument(profile)+' --source-sha '+sha+
    (checkout?' --checkout '+argument(checkout):'')+(file?' --file '+argument(file):'')+(pluginId?' --plugin-id '+argument(pluginId):'');
}
