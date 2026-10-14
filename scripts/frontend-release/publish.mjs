import { canonicalManifest, checksum } from './manifest.mjs';
import { verifySignature } from './sign.mjs';

export async function publishRelease({ manifest, signature, archive, publicKey, dryRun, fixtureKey }, remote) {
  const canonical = canonicalManifest(manifest);
  verifySignature(canonical, signature, publicKey);
  if (checksum(archive) !== manifest.sha256) throw new Error('Archive checksum mismatch');
  if (dryRun) return { published: false };
  if (!publicKey || publicKey.trim().split('\n').at(-1) === fixtureKey.trim().split('\n').at(-1)) throw new Error('Fixture identity cannot publish');
  const published = await remote.createDraft(manifest.releaseId);
  const assets = [[`${manifest.releaseId}.tar.gz`, archive], ['frontend-manifest.json', canonical], ['frontend-manifest.json.minisig', Buffer.from(signature)]];
  if (!published) for (const [name, bytes] of assets) await remote.upload(manifest.releaseId, name, bytes);
  for (const [name, bytes] of assets) {
    const uploaded = await remote.download(manifest.releaseId, name);
    if (!Buffer.from(uploaded).equals(bytes)) throw new Error(`Uploaded asset mismatch: ${name}`);
  }
  if (!published) await remote.publish(manifest.releaseId);
  await remote.discover(manifest.releaseId, canonical, Buffer.from(signature));
  return { published: true };
}
