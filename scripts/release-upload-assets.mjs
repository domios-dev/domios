#!/usr/bin/env node
// Upload release assets and say exactly what happened. tauri-action's uploader
// reports a bare "Error saving asset" with no status, body or asset name, which
// cost 1.11.3 four runs before the failing asset was even identified.
//
// Every upload here names the asset, its size, the HTTP status and the response
// body on failure, retries a transfer that died mid-stream, and verifies the
// asset afterwards: GitHub reports `state: "starter"` for a partial upload, and
// such an asset holds the name while never appearing in a normal listing.
//
// Usage: node scripts/release-upload-assets.mjs <owner/repo> <tag> <file...>
// Requires GH_TOKEN or GITHUB_TOKEN.

import { createReadStream, statSync } from "node:fs";
import { basename } from "node:path";

const [repo, tag, ...files] = process.argv.slice(2);
if (!repo || !tag || files.length === 0) {
  console.error("usage: release-upload-assets.mjs <owner/repo> <tag> <file...>");
  process.exit(2);
}

const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
if (!token) {
  console.error("GH_TOKEN or GITHUB_TOKEN is required");
  process.exit(2);
}

const ATTEMPTS = 3;
const headers = {
  accept: "application/vnd.github+json",
  authorization: `Bearer ${token}`,
  "x-github-api-version": "2022-11-28",
};

const api = async (path, init = {}) => {
  const response = await fetch(`https://api.github.com${path}`, {
    ...init,
    headers: { ...headers, ...(init.headers ?? {}) },
  });
  const body = await response.text();
  if (!response.ok) {
    throw new Error(`${init.method ?? "GET"} ${path} -> HTTP ${response.status}: ${body.slice(0, 400)}`);
  }
  return body ? JSON.parse(body) : null;
};

const findRelease = async () => {
  try {
    return await api(`/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`);
  } catch {
    const releases = await api(`/repos/${repo}/releases?per_page=100`);
    return releases.find((release) => release.tag_name === tag) ?? null;
  }
};

const release = await findRelease();
if (!release) {
  console.error(`no release ${tag} on ${repo}`);
  process.exit(1);
}

const assetsFor = async () => api(`/repos/${repo}/releases/${release.id}/assets?per_page=100`);

// A name is free only when nothing holds it, uploaded or half-uploaded.
const freeName = async (name) => {
  for (const asset of await assetsFor()) {
    if (asset.name !== name) continue;
    await api(`/repos/${repo}/releases/assets/${asset.id}`, { method: "DELETE" });
    console.log(`  removed existing ${name} (state=${asset.state})`);
  }
};

const uploadOnce = async (file, name, size) => {
  const url = `${release.upload_url.replace(/\{\?[^}]*\}$/, "")}?name=${encodeURIComponent(name)}`;
  const response = await fetch(url, {
    method: "POST",
    headers: { ...headers, "content-type": "application/octet-stream", "content-length": String(size) },
    body: createReadStream(file),
    duplex: "half",
  });
  const body = await response.text();
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${body.slice(0, 400)}`);
  }
  return JSON.parse(body);
};

let failed = 0;
for (const file of files) {
  const name = basename(file);
  const size = statSync(file).size;
  console.log(`uploading ${name} (${size} bytes) to ${repo} ${tag}`);
  let landed = false;
  for (let attempt = 1; attempt <= ATTEMPTS && !landed; attempt += 1) {
    try {
      await freeName(name);
      const asset = await uploadOnce(file, name, size);
      const [verified] = (await assetsFor()).filter((candidate) => candidate.id === asset.id);
      if (!verified) {
        throw new Error("asset vanished immediately after upload");
      }
      if (verified.state !== "uploaded") {
        throw new Error(`asset settled in state=${verified.state}, not uploaded`);
      }
      if (verified.size !== size) {
        throw new Error(`asset is ${verified.size} bytes, expected ${size}`);
      }
      console.log(`  ok ${name}: state=${verified.state} size=${verified.size}`);
      landed = true;
    } catch (error) {
      console.error(`  attempt ${attempt}/${ATTEMPTS} failed for ${name}: ${error.message}`);
    }
  }
  if (!landed) {
    failed += 1;
    console.error(`::error::${name} did not upload after ${ATTEMPTS} attempts`);
  }
}
process.exit(failed === 0 ? 0 : 1);
