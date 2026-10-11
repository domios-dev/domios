const childProcess = require('node:child_process');
const { syncBuiltinESMExports } = require('node:module');
const { basename } = require('node:path');
const spawn = childProcess.spawn;

function guiEnvironment(source) {
  const env = {
    PATH: '/usr/bin:/bin:/usr/sbin:/sbin',
    HOME: source.HOME,
    USER: source.USER,
  };
  for (const [key, value] of Object.entries(source)) {
    if (key.startsWith('CODETERM_')) env[key] = value;
  }
  return env;
}

if (process.env.RL_GUI_MODE === '1') {
  childProcess.spawn = function (command, args, options) {
    // Keep build.sh's Node tooling usable; restrict only the actual app child.
    if (basename(command) === 'codeterm' && args.length === 0) {
      options = { ...options, env: guiEnvironment(options?.env ?? process.env) };
    }
    return spawn.call(this, command, args, options);
  };
  syncBuiltinESMExports();
}

module.exports = { guiEnvironment };
