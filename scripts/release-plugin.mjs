// Caller: CI plugin-release.yml, tests.yml; scripts ci-scope.mjs, tests/ci-scope.test.mjs; skill github-release
import path from "node:path";
import { bumpPlugin, channelPlugins, checkPluginTag } from "./plugin-release-metadata.mjs";

const root = path.resolve(import.meta.dirname, "..");
const [command, idOrTag, version, flag, minimum, ...extra] = process.argv.slice(2);

try {
  if (command === "check" && !version) {
    if (idOrTag) {
      const plugin = checkPluginTag(root, idOrTag);
      console.log(JSON.stringify({ id: plugin.id, path: plugin.path, version: plugin.version, minCodeterm: plugin.minCodeterm }));
    } else {
      console.log(`Plugin metadata agrees (${channelPlugins(root).length} checked)`);
    }
  } else if (command === "bump" && idOrTag && version && !extra.length &&
             ((!flag && !minimum) || (flag === "--min-codeterm" && minimum))) {
    bumpPlugin(root, idOrTag, version, minimum);
    console.log(`Prepared ${idOrTag} ${version}; app version unchanged`);
  } else {
    throw new Error("Usage: node scripts/release-plugin.mjs check [plugin-ID-vVERSION] | bump ID VERSION [--min-codeterm VERSION]");
  }
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
