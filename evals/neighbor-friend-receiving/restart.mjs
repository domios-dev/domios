import { spawn } from "node:child_process";
import { join } from "node:path";
import { authorityTabs, Peer, ready, receiving, requireThat, retired, topology } from "./peers.mjs";
import { loadManifest, qualifyPeer, repo } from "./qualification.mjs";

async function restart() {
  const [manifestPath, expectedBoot] = process.argv.slice(2);
  const manifest = loadManifest(manifestPath);
  requireThat(typeof expectedBoot === "string" && /^[a-zA-Z0-9-]{16,80}$/.test(expectedBoot), "Expected C boot identity required");
  const peers = Object.fromEntries(Object.keys(topology).map(label => [label, new Peer(label)]));
  await Promise.all(Object.values(peers).map(peer => qualifyPeer(peer, manifest)));
  requireThat(peers.C.boot === expectedBoot, "C changed since restart was requested");
  const [B, C] = await Promise.all([peers.B.snapshot(), peers.C.snapshot()]);
  requireThat(B.runtime.receiving_here === true && C.runtime.receiving_here === false &&
    receiving(B, manifest.friend_x, true, "account") && receiving(C, manifest.friend_x, false, "account") &&
    receiving(B, manifest.friend_y, true, "account") && receiving(C, manifest.friend_y, false, "account") &&
    ready(B, manifest.friend_x) && ready(B, manifest.friend_y) && retired(C, manifest.friend_x) &&
    authorityTabs(C, manifest.friend_y).length === 0 && !C.runtime.bindings?.[manifest.friend_y],
    "Restart requires the manifest's reset/dormant nonreceiver fixture");
  const proofs = await Promise.all(Object.values(peers).map(peer => qualifyPeer(peer, manifest)));
  console.log(JSON.stringify({ phase: "qualified_before_shutdown", peers: proofs }));
  const command = `set -euo pipefail
source "$1"
dev_assert_target
dev_assert_restart_target 0
cd "$2"
dev_run bash build.sh kill
dev_run bash build.sh dev-prebuilt --wait`;
  const bash = process.platform === "win32" ? "C:/Program Files/Git/bin/bash.exe" : "bash";
  const code = await new Promise((accept, reject) => {
    const child = spawn(bash, ["-c", command, "per-friend-restart", join(repo, "evals/_lib/lib.sh"), repo], {
      cwd: repo, windowsHide: true, stdio: "inherit", signal: AbortSignal.timeout(240_000),
      env: { ...process.env, CODETERM_INSTANCE: "curated-second", CODETERM_API_PORT: "7717",
        DOMIOS_ACCOUNT_TARGET: "qa", CODETERM_DEV_WATCH: "0" },
    });
    child.once("error", () => reject(new Error("Qualified C launcher failed")));
    child.once("close", accept);
  });
  requireThat(code === 0, `Qualified C launcher exit ${code}; no retry`);
}

try { await restart(); }
catch (error) {
  console.error(error instanceof Error ? error.message.slice(0, 300) : "C restart qualification failed");
  process.exitCode = 1;
}
