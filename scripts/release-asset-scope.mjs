export function ownsReleaseAsset(name, platform) {
  const suffixes = {
    'frontend': /^(?:frontend-v[0-9]+\.[0-9]+\.[0-9]+\.tar\.gz|frontend-manifest\.json(?:\.minisig)?)$/,
    'darwin-aarch64': /(?:aarch64\.app\.tar\.gz|aarch64\.dmg|updater-darwin-aarch64\.json)$/,
    'darwin-x86_64': /(?:x64\.app\.tar\.gz|x64\.dmg|updater-darwin-x86_64\.json)$/,
    'linux-aarch64': /(?:aarch64\.AppImage|arm64\.deb|updater-linux-aarch64(?:-deb)?\.json)$/,
    'linux-x86_64': /(?:amd64\.(?:AppImage|deb)|updater-x86_64-unknown-linux-gnu(?:-deb)?\.json)$/,
    'windows-x86_64': /(?:x64-setup\.exe|updater-x86_64-pc-windows-msvc\.json)$/,
    'windows-aarch64': /(?:arm64-setup\.exe|updater-aarch64-pc-windows-msvc\.json)$/,
  };
  if (!suffixes[platform]) throw new Error(`Unknown upload scope ${platform}`);
  return suffixes[platform].test(name);
}

