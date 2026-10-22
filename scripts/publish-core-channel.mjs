// Caller: CI plugin-release.yml; scripts tests/core-channel-publication.test.mjs; skill github-release
import { execFileSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { archiveRuntimeFiles } from "./plugin-runtime-files.mjs";

export const CORE_CHANNEL_RELEASE = "codeterm-core-latest";
export const CORE_CHANNEL_ARCHIVE = "codeterm-core.tar.gz";
const SOURCE_COMMIT = "codeterm-core.source-commit";

/** Never let an older release tag overwrite a channel published from newer master history. */
export function publicationDecision(previousSha, sourceSha, isAncestor) {
  if (!previousSha) return "publish";
  if (previousSha === sourceSha) return "skip";
  if (isAncestor(previousSha, sourceSha)) return "publish";
  if (isAncestor(sourceSha, previousSha)) return "skip";
  throw new Error(`published core channel ${previousSha} does not share history with ${sourceSha}`);
}

export function checksumLine(bytes, name) {
  return `${crypto.createHash("sha256").update(bytes).digest("hex")}  ${name}\n`;
}

export function packageChannel(sourceSha, directory, run = execFileSync) {
  const archive = path.join(directory, CORE_CHANNEL_ARCHIVE);
  archiveRuntimeFiles(sourceSha, "plugins", archive, run);
  fs.writeFileSync(`${archive}.sha256`, checksumLine(fs.readFileSync(archive), CORE_CHANNEL_ARCHIVE));
  fs.writeFileSync(path.join(directory, SOURCE_COMMIT), `${sourceSha}\n`);
  return [archive, `${archive}.sha256`, path.join(directory, SOURCE_COMMIT)];
}

function isAncestor(older, newer) {
  try {
    execFileSync("git", ["merge-base", "--is-ancestor", older, newer], { stdio: "ignore" });
    return true;
  } catch { return false; }
}

async function publish(sourceSha, repository) {
  const gh = args => execFileSync("gh", args, { stdio: ["ignore", "pipe", "pipe"], timeout: 120000 }).toString();
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "codeterm-core-channel-"));
  const files = packageChannel(sourceSha, directory);
  let previous = null;
  try {
    gh(["release", "view", CORE_CHANNEL_RELEASE, "--repo", repository]);
    previous = gh(["release", "download", CORE_CHANNEL_RELEASE, "--repo", repository, "--pattern", SOURCE_COMMIT, "--output", "-"]).trim() || null;
  } catch {
    gh(["release", "create", CORE_CHANNEL_RELEASE, "--repo", repository, "--latest=false",
      "--title", "codeterm-core plugin channel", "--notes", "Official core plugin channel, published from verified plugin release tags."]);
  }
  if (publicationDecision(previous, sourceSha, isAncestor) === "skip") {
    console.log(`Public core channel already carries ${previous}; ${sourceSha} not published`);
    return;
  }
  gh(["release", "upload", CORE_CHANNEL_RELEASE, ...files, "--clobber", "--repo", repository]);
  const url = `https://github.com/${repository}/releases/download/${CORE_CHANNEL_RELEASE}/${CORE_CHANNEL_ARCHIVE}`;
  const response = await fetch(url, { cache: "no-store" });
  if (!response.ok) throw new Error(`anonymous download of ${url} failed: HTTP ${response.status}`);
  const served = checksumLine(Buffer.from(await response.arrayBuffer()), CORE_CHANNEL_ARCHIVE);
  if (served !== fs.readFileSync(files[1], "utf8")) throw new Error(`${url} does not serve the uploaded archive`);
  console.log(`Published codeterm-core channel ${sourceSha} to ${url}`);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [command, sourceSha, repository] = process.argv.slice(2);
  if (command !== "publish" || !/^[0-9a-f]{40}$/.test(sourceSha ?? "") || !repository) {
    console.error("Usage: node scripts/publish-core-channel.mjs publish SOURCE_SHA OWNER/REPOSITORY");
    process.exit(1);
  }
  publish(sourceSha, repository).catch(error => { console.error(error.message); process.exit(1); });
}
