import { installers, isInstallerUrl, releasesPage } from './catalog';
import type { DownloadInfo, Installer } from './catalog';

const latestRelease = 'https://api.github.com/repos/rollacode/codeterm-releases/releases/latest';
const cacheMilliseconds = 300_000;
let cached: { info: DownloadInfo; expires: number } | undefined;
let pending: Promise<DownloadInfo> | undefined;

export const releaseCache = 'public, max-age=0, s-maxage=300, stale-while-revalidate=600';
export const fallbackCache = 'public, max-age=0, s-maxage=60, stale-while-revalidate=300';

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

function matchesAsset(name: string, installer: Installer) {
  const suffix = installer.suffix.replaceAll('.', '\\.');
  return new RegExp(`^CodeTerm_[0-9]+\\.[0-9]+\\.[0-9]+(?:-[A-Za-z0-9.-]+)?_${suffix}$`).test(name);
}

export function releaseInfo(release: unknown): DownloadInfo {
  const assets = record(release) && Array.isArray(release.assets) ? release.assets : [];
  const tag = record(release) && typeof release.tag_name === 'string' ? release.tag_name : null;
  return {
    version: tag,
    downloads: installers.map(installer => {
      const asset = assets.find((value: unknown) => record(value) && typeof value.name === 'string'
        && matchesAsset(value.name, installer) && typeof value.browser_download_url === 'string'
        && isInstallerUrl(value.browser_download_url));
      return {
        id: installer.id,
        href: record(asset) ? String(asset.browser_download_url) : releasesPage,
        available: Boolean(asset),
        asset: record(asset) ? String(asset.name) : null,
      };
    }),
  };
}

export function getReleaseInfo(): Promise<DownloadInfo> {
  if (cached && cached.expires > Date.now()) return Promise.resolve(cached.info);
  if (pending) return pending;
  pending = fetch(latestRelease, {
    headers: { Accept: 'application/vnd.github+json', 'User-Agent': 'domios-download', 'X-GitHub-Api-Version': '2022-11-28' },
    signal: AbortSignal.timeout(5000),
  }).then(async response => {
    if (!response.ok) throw new Error('Release lookup unavailable');
    const info = releaseInfo(await response.json());
    cached = { info, expires: Date.now() + cacheMilliseconds };
    return info;
  }).catch(() => {
    const info = cached?.info ?? releaseInfo(null);
    cached = { info, expires: Date.now() + 60_000 };
    return info;
  }).finally(() => { pending = undefined; });
  return pending;
}
