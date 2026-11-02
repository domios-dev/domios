import { createHash } from 'node:crypto';

export const fields = ['maxApiVersion', 'minApiVersion', 'releaseId', 'sha256', 'url', 'version'];
const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$(?![\s\S])/;
export function validateTag(tag, contract) {
  if (!tag.startsWith('frontend-v') || !semver.test(tag.slice(10)) || tag.slice(10) !== contract.version) {
    throw new Error('Frontend tag must match independent frontend metadata');
  }
  return tag.slice(10);
}
export function validateManifest(value) {
  const keys = value && Object.keys(value).filter(key => key !== 'releaseNotes').sort();
  if (!keys || keys.join() !== fields.join()) throw new Error('Unexpected manifest fields');
  if (value.releaseNotes !== undefined && (typeof value.releaseNotes !== 'string' || Buffer.byteLength(value.releaseNotes, 'utf8') > 4096)) throw new Error('Invalid frontend release notes');
  if (typeof value.version !== 'string' || !semver.test(value.version)) throw new Error('Invalid frontend version');
  if (value.releaseId !== `frontend-v${value.version}`) throw new Error('Invalid release identity');
  if (!Number.isSafeInteger(value.minApiVersion) || value.minApiVersion < 1 ||
      !Number.isSafeInteger(value.maxApiVersion) || value.maxApiVersion < value.minApiVersion) throw new Error('Invalid API range');
  if (typeof value.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(value.sha256)) throw new Error('Invalid archive digest');
  const url = new URL(value.url);
  if (url.protocol !== 'https:' || url.username || url.password || url.hash || url.search) throw new Error('Invalid archive URL');
  return value;
}
export function canonicalManifest(value) {
  validateManifest(value);
  return Buffer.from(JSON.stringify(Object.fromEntries(Object.keys(value).sort().map(key => [key, value[key]]))), 'utf8');
}
export function checksum(bytes) { return createHash('sha256').update(bytes).digest('hex'); }
export function createManifest(tag, contract, archive) {
  const version = validateTag(tag, contract);
  return validateManifest({ version, minApiVersion: contract.minApiVersion, maxApiVersion: contract.maxApiVersion,
    sha256: checksum(archive), releaseId: tag, ...(contract.releaseNotes === undefined ? {} : { releaseNotes: contract.releaseNotes }),
    url: `https://github.com/${contract.repository}/releases/download/${tag}/${tag}.tar.gz` });
}
