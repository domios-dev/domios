import fs from 'node:fs';
import path from 'node:path';

export const runnerProfiles=JSON.parse(fs.readFileSync(new URL('./ci-runner-profiles.json',import.meta.url),'utf8'));
export function runnerProfile(name) {
  const profile=runnerProfiles.profiles[name];
  if(!profile)throw new Error('Unknown runner profile: '+name);
  return profile;
}
export function inferRunnerProfile(env=process.env,platform=process.platform) {
  if(env.CI_RUNNER_PROFILE){runnerProfile(env.CI_RUNNER_PROFILE);return env.CI_RUNNER_PROFILE;}
  if(env.RUNNER_ENVIRONMENT==='github-hosted')return platform==='win32'?'hosted-windows':platform==='darwin'?'hosted-macos':'hosted-linux';
  return platform==='win32'?'rog-service':platform==='darwin'?'mac-service':'ubunta-service';
}
export function profileEnvironment(name,base,directory,{platform=process.platform,toolPaths=[],nodeDirectory=path.dirname(process.execPath)}={}) {
  const profile=runnerProfile(name);
  const env={...base,CI:'true',RUNNER_ENVIRONMENT:profile.environment,CI_RUNNER_PROFILE:name,
    CARGO_HOME:path.join(directory,'cargo-home'),RUNNER_TEMP:directory,
    CODETERM_RUST_CACHE_ROOT:path.join(directory,'rust-cache'),CODETERM_USE_SCCACHE:'0',
    CODETERM_SCCACHE_VERIFY_ONLY:'0',RUSTC_WRAPPER:'',RUSTC_WORKSPACE_WRAPPER:'',
    CI_ARTIFACT_STORAGE:'github',GITHUB_PATH:path.join(directory,'github-path'),GITHUB_ENV:path.join(directory,'github-env'),
    CI_REPRO_PLATFORM:profile.platform,CI_REPRO_PYTHON_COMMAND:profile.python,
  };
  for(const key of Object.keys(env))if(/^(?:AWS_|SCCACHE_|DOMIOS_|TAURI_SIGNING_|APPLE_|GH_TOKEN$|GITHUB_TOKEN$|CARGO_TARGET_DIR$|CARGO_BUILD_BUILD_DIR$|CI_TARGET_ROOT$|CI_PYTHON$|VIRTUAL_ENV$)/.test(key))delete env[key];
  const system=platform==='win32'
    ? [path.join(base.SystemRoot||'C:/Windows','System32'),base.SystemRoot].filter(Boolean)
    : ['/usr/local/bin','/usr/bin','/bin',...(platform==='darwin'?['/opt/homebrew/bin']:[])];
  env.PATH=[nodeDirectory,...toolPaths,...system].filter(Boolean).join(platform==='win32'?';':':');
  return env;
}
