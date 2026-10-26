import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {bashInterpreter,checkBashTools,checkRunnerTools} from './ci-runner-paths.mjs';

import { runnerProfiles } from './ci-runner-profile.mjs';
export const PREFLIGHT_PROFILES=Object.fromEntries(Object.entries(runnerProfiles.preflight).map(([name,value])=>[name,value.tools]));
const profile=process.argv[2];
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
 if(!PREFLIGHT_PROFILES[profile])throw new Error('Unknown preflight profile: '+profile);
 checkRunnerTools(PREFLIGHT_PROFILES[profile].join(','));
 if(process.platform==='win32')checkBashTools(PREFLIGHT_PROFILES[profile].join(','),bashInterpreter());
 console.log('Preflight passed: '+profile+' ('+PREFLIGHT_PROFILES[profile].join(',')+')');
}
