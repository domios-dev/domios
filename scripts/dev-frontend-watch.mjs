import { randomUUID } from "node:crypto";
import { existsSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { build } from "vite";

const [outDir, statusPath] = process.argv.slice(2);
const indexPath = join(outDir, "index.html");
const initialIndex = existsSync(indexPath) ? readFileSync(indexPath) : null;
let revision = "initial";
let built = false;
let recovering = false;

function publish(failed) {
  const pending = statusPath + ".tmp";
  writeFileSync(pending, JSON.stringify({ revision, failed }));
  renameSync(pending, statusPath);
}

function fail(error) {
  publish(true);
  console.error("[dev-watch]", error);
  process.exitCode = 1;
}

async function startWatching() {
  const watcher = await build({ build: { watch: {}, emptyOutDir: false, outDir } });
  watcher.on("event", event => {
    if (event.code === "BUNDLE_END") {
      if (built || !initialIndex || !readFileSync(indexPath).equals(initialIndex)) revision = randomUUID();
      built = true;
      recovering = false;
      publish(false);
    } else if (event.code === "ERROR") {
      publish(true);
      const retry = !recovering && event.error.message.includes("Unable to get file name for unknown file");
      recovering = retry;
      if (retry) {
        console.log("[dev-watch] Restarting with a clean Vite build after an invalid asset reference");
        void watcher.close().then(startWatching).catch(fail);
      }
    }
  });
}

await startWatching().catch(fail);
