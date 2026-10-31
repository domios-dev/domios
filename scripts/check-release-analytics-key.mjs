// Caller: release-linux.yml, release-linux-arm64.yml, release-macos-build.yml, release-windows.yml, release-windows-arm64.yml
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const marker = 'domios-amplitude-prod:';

export function amplitudeFingerprint(key) {
  return key?.trim() ? createHash('sha256').update(key).digest('hex').slice(0, 8) : 'none';
}

export function verifyAnalyticsKey(bytes, key) {
  const fingerprint = amplitudeFingerprint(key);
  if (fingerprint === 'none') throw new Error('Release analytics production key is missing');
  if (bytes.includes(Buffer.from(marker + 'none')) || !bytes.includes(Buffer.from(marker + fingerprint))) {
    throw new Error('Built server analytics fingerprint differs from the release production key');
  }
  return fingerprint;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    if (process.argv.length !== 3) throw new Error('Usage: node scripts/check-release-analytics-key.mjs SERVER_BINARY');
    const fingerprint = verifyAnalyticsKey(readFileSync(process.argv[2]), process.env.DOMIOS_AMPLITUDE_API_KEY_PROD);
    console.log(`Release analytics production fingerprint verified: ${fingerprint}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
