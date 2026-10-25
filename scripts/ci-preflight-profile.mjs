import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {checkRunnerTools} from './ci-runner-paths.mjs';

export const PREFLIGHT_PROFILES={
 'hosted-linux':['bash','cargo','curl','git','jq','nextest','node','pnpm','python','python:yaml','tar'],
 'hosted-windows':['bash','git','node','pnpm'],
 'hosted-macos':['bash','cargo','curl','git','nextest','node','python'],
 'self-linux-ubunta-light':['bash','curl','git','node'],
 'self-linux-ubunta-compile':['bash','cargo','curl','git','nextest','node','python'],
 'self-macos':['bash','cargo','curl','git','nextest','node','python'],
 'self-windows':['bash','cargo','curl','git','nextest','node','python'],
};
const profile=process.argv[2];
if(process.argv[1]&&import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href){
 if(!PREFLIGHT_PROFILES[profile])throw new Error('Unknown preflight profile: '+profile);
 checkRunnerTools(PREFLIGHT_PROFILES[profile].join(','));
 console.log('Preflight passed: '+profile+' ('+PREFLIGHT_PROFILES[profile].join(',')+')');
}
