import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { requireThat, topology } from "./peers.mjs";

export const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const digest = value => createHash("sha256").update(value).digest("hex");

export function loadManifest(manifestPath) {
  requireThat(process.env.DOMIOS_LIVE_PER_FRIEND === "1", "Live bench opt-in required");
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  requireThat(manifest.schema === 1 && manifest.control_settled === true, "Explicit settled-control acknowledgement required");
  requireThat(/^[a-f0-9]{40}$/.test(manifest.candidate_sha), "Pin a full candidate SHA");
  requireThat(/^[a-f0-9]{40}$/.test(manifest.qa_backend?.functions_sha) &&
    manifest.qa_backend.target === "qa" && manifest.qa_backend.project_id === "domios-signin-qa",
    "Pin the authorized QA backend attestation");
  requireThat(/^[a-f0-9]{64}$/.test(manifest.friend_x) && /^[a-f0-9]{64}$/.test(manifest.friend_y) &&
    manifest.friend_x !== manifest.friend_y, "Two distinct provisioned friendships are required");
  requireThat(Number.isInteger(manifest.deadline_seconds) && manifest.deadline_seconds >= 30 && manifest.deadline_seconds <= 180,
    "Observation deadline must be 30..180 seconds");
  for (const label of Object.keys(topology)) {
    requireThat(/^[a-f0-9]{64}$/.test(manifest.qa_identity_sha256?.[label]), `${label}: QA identity hash required`);
  }
  requireThat(manifest.qa_identity_sha256.B === manifest.qa_identity_sha256.C &&
    new Set([manifest.qa_identity_sha256.A, manifest.qa_identity_sha256.B, manifest.qa_identity_sha256.D]).size === 3,
    "B/C must share the Peer account; A and D must be independent QA friends");
  requireThat(execFileSync("git", ["-C", repo, "rev-parse", "HEAD"], { encoding: "utf8", windowsHide: true }).trim() === manifest.candidate_sha,
    "Candidate checkout differs from the pinned build");
  requireThat(execFileSync("git", ["-C", repo, "status", "--porcelain"], { encoding: "utf8", windowsHide: true }).trim() === "",
    "Candidate checkout is dirty");
  return manifest;
}

export function requireActualQa(backend, manifest) {
  requireThat(backend?.schema_version === 1 && backend.target === "qa" &&
    backend.project_id === "domios-signin-qa" && backend.project_id === manifest.qa_backend.project_id &&
    backend.functions_endpoint_kind === "qa", "Actual QA backend qualification is missing or mismatched");
  return { target: backend.target, project_id: backend.project_id, functions_endpoint_kind: backend.functions_endpoint_kind };
}

export async function qualifyPeer(peer, manifest) {
  const health = await peer.api("/api/health");
  const [account, backend] = await Promise.all([peer.api("/api/account/status"), peer.api("/api/account/backend")]);
  const actualBackend = requireActualQa(backend, manifest);
  const after = await peer.api("/api/health");
  requireThat(health.status === "ok" && health.startup?.state === "ready" && health.build_sha === manifest.candidate_sha &&
    health.instance?.name === peer.target.instance && Number(health.instance.api_port) === peer.target.api &&
    Number(health.instance.vite_port) === peer.target.frontend && Number(health.instance.cdp_port) === peer.target.cdp &&
    typeof health.boot_id === "string" && health.boot_id.length > 0 && after.boot_id === health.boot_id &&
    after.build_sha === health.build_sha && after.instance?.name === health.instance.name &&
    Number(after.instance.api_port) === peer.target.api, `${peer.label}: candidate/instance/readiness changed during qualification`);
  requireThat(account.provider === "firebase" && account.pending === false && !account.session_revoked &&
    typeof account.identity?.uid === "string" && digest(account.identity.uid) === manifest.qa_identity_sha256[peer.label],
    `${peer.label}: not the pinned signed-in QA identity`);
  if (peer.boot) requireThat(health.boot_id === peer.boot, `${peer.label}: unexpected restart`);
  peer.boot = health.boot_id;
  return { build_sha: health.build_sha, boot_id: health.boot_id, instance: peer.target.instance,
    qa_identity_matches: true, actual_backend: actualBackend };
}
