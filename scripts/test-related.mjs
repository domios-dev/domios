import {bashInterpreter} from './bash-interpreter.mjs';
import { spawnSync } from "node:child_process";
import { relatedSelection } from "./affected-tests.mjs";
import { pathToFileURL } from "node:url";

function gateCommand(args) {
  return "./build.sh " + args.map(arg => /^[a-zA-Z0-9_./:=+-]+$/.test(arg)
    ? arg : `'${arg.replaceAll("'", "'\\''")}'`).join(" ");
}

export function executeRelatedSelection(commands, { run = spawnSync, write = console.log,
  shell = process.env.CT_TEST_RELATED_SHELL || bashInterpreter() } = {}) {
  for (const command of commands) write(command[0] === "./build.sh"
    ? gateCommand(command.slice(1)) : command.map(arg => `'${arg.replaceAll("'", "'\\''")}'`).join(" "));
  for (const command of commands) {
    const result = run(shell, ["-c", 'exec "$@"', "test-related", ...command], { stdio: "inherit" });
    if (result.error) throw result.error;
    if (result.status !== 0) return result.status ?? 1;
  }
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) try {
  process.exitCode = executeRelatedSelection(relatedSelection(process.argv.slice(2)));
} catch (error) {
  console.error(`test-related: ${error.message}`);
  process.exitCode = 2;
}
