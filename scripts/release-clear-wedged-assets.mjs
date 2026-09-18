#!/usr/bin/env node
// A GitHub asset upload that dies mid-transfer leaves the asset in `starter`
// state. It holds the name, never appears in a normal asset listing, and makes
// every later upload of that name fail with the opaque "Error saving asset" —
// including tauri-action's, whose own delete does not remove it. So one dropped
// connection wedges a release permanently until someone clears it by hand.
//
// A `starter` asset is by definition a failed partial upload and is never
// wanted, so this clears all of them on a release before the upload steps run.
//
// Usage: node scripts/release-clear-wedged-assets.mjs <owner/repo> <tag>
// Requires GH_TOKEN or GITHUB_TOKEN. Exits 0 when there is nothing to clear,
// and never fails the build for a missing release — it is a pre-clean, not a gate.

const [repo, tag] = process.argv.slice(2);
if (!repo || !tag) {
  console.error("usage: release-clear-wedged-assets.mjs <owner/repo> <tag>");
  process.exit(2);
}

const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN;
if (!token) {
  console.error("GH_TOKEN or GITHUB_TOKEN is required");
  process.exit(2);
}

const api = async (path, init = {}) => {
  const response = await fetch(`https://api.github.com${path}`, {
    ...init,
    headers: {
      accept: "application/vnd.github+json",
      authorization: `Bearer ${token}`,
      "x-github-api-version": "2022-11-28",
      ...(init.headers ?? {}),
    },
  });
  if (!response.ok) {
    throw new Error(`${init.method ?? "GET"} ${path} -> HTTP ${response.status}`);
  }
  return response.status === 204 ? null : response.json();
};

// A draft release is absent from /releases/tags/<tag>, so scan the list too.
const findRelease = async () => {
  try {
    return await api(`/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`);
  } catch {
    const releases = await api(`/repos/${repo}/releases?per_page=100`);
    return releases.find((release) => release.tag_name === tag) ?? null;
  }
};

const release = await findRelease().catch((error) => {
  console.log(`no release to pre-clean for ${repo} ${tag}: ${error.message}`);
  return null;
});
if (!release) {
  console.log(`no release ${tag} on ${repo} yet; nothing to pre-clean`);
  process.exit(0);
}

const assets = await api(`/repos/${repo}/releases/${release.id}/assets?per_page=100`);
const wedged = assets.filter((asset) => asset.state !== "uploaded");
if (wedged.length === 0) {
  console.log(`${repo} ${tag}: no wedged assets`);
  process.exit(0);
}

let failed = 0;
for (const asset of wedged) {
  try {
    await api(`/repos/${repo}/releases/assets/${asset.id}`, { method: "DELETE" });
    console.log(`cleared wedged asset ${asset.name} (state=${asset.state})`);
  } catch (error) {
    failed += 1;
    console.error(`could not clear ${asset.name}: ${error.message}`);
  }
}
process.exit(failed === 0 ? 0 : 1);
