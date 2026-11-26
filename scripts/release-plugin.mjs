// Caller: CI plugin-release.yml, tests-fast.yml; scripts ci-scope.mjs, tests/ci-scope.test.mjs; skill github-release
import path from "node:path";
import { execFileSync } from "node:child_process";
import { archiveRuntimeFiles, runtimeFiles } from "./plugin-runtime-files.mjs";
import { bumpPlugin, channelPlugins, checkPluginTag } from "./plugin-release-metadata.mjs";

const root = path.resolve(import.meta.dirname, "..");
const args = process.argv.slice(2);
const external = args[0] === "--channel-root";
const channelDirectory = external && args[1] ? path.resolve(args[1]) : undefined;
const [command, idOrTag, version, flag, minimum, ...extra] = external ? args.slice(2) : args;

try {
  if (external && !channelDirectory) throw new Error("--channel-root requires a channel directory");
  if (command === "check" && !version) {
    if (idOrTag) {
      const plugin = checkPluginTag(root, idOrTag, channelDirectory);
      runtimeFiles(plugin.directory);
      console.log(JSON.stringify({ id: plugin.id, path: plugin.path, version: plugin.version, minCodeterm: plugin.minCodeterm }));
    } else {
      const plugins = channelPlugins(root, channelDirectory);
      for (const plugin of plugins) runtimeFiles(plugin.directory);
      console.log(`Plugin metadata agrees (${plugins.length} checked)`);
    }
  } else if (!external && command === "archive" && idOrTag && version && flag && !minimum) {
    const plugin = checkPluginTag(root, idOrTag);
    archiveRuntimeFiles(version, `plugins/${plugin.path}`, flag, execFileSync);
  } else if (command === "bump" && idOrTag && version && !extra.length &&
             ((!flag && !minimum) || (flag === "--min-codeterm" && minimum))) {
    bumpPlugin(root, idOrTag, version, minimum, channelDirectory);
    console.log(`Prepared ${idOrTag} ${version}; app version unchanged`);
  } else {
    throw new Error("Usage: node scripts/release-plugin.mjs [--channel-root DIRECTORY] check [plugin-ID-vVERSION] | archive TAG SHA OUTPUT | bump ID VERSION [--min-codeterm VERSION]");
  }
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
