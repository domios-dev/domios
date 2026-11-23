const releaseDownloads = 'https://github.com/domios-dev/domios/releases/download';
const installerSuffixes = ['aarch64.dmg', 'x64-setup.exe', 'amd64.deb', 'amd64.AppImage'];
const installerLink = /(<a\s+href=")https:\/\/github\.com\/domios-dev\/domios\/releases\/download\/v[^/"\s]+\/Domios_[^_/"\s]+_([^/"\s]+)"/g;

export function updateReadmeDownloads(readme, version) {
  const missing = new Set(installerSuffixes);
  const updated = readme.replace(installerLink, (link, opening, suffix) => {
    if (!installerSuffixes.includes(suffix)) return link;
    if (!missing.delete(suffix)) throw new Error(`README.md has duplicate download links for ${suffix}`);
    return `${opening}${releaseDownloads}/v${version}/Domios_${version}_${suffix}"`;
  });
  if (missing.size) throw new Error(`README.md is missing download links for ${[...missing].join(', ')}`);
  return updated;
}
