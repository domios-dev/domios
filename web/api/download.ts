import { installers, isInstallerUrl, releasesPage } from '../src/download/catalog';
import type { DownloadInfo } from '../src/download/catalog';
import { releaseCache, fallbackCache } from '../src/download/release';

export const config = { runtime: 'edge', regions: ['iad1'] };

export default async function handler(request: Request) {
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    return new Response(null, { status: 405, headers: { Allow: 'GET, HEAD' } });
  }
  const { searchParams } = new URL(request.url);
  const installer = installers.find(value => value.os === searchParams.get('os')
    && value.arch === searchParams.get('arch') && value.format === searchParams.get('format'));
  let location = releasesPage;
  if (installer) {
    try {
      const response = await fetch(new URL('/api/download/info', request.url), { signal: AbortSignal.timeout(7000) });
      if (response.ok) {
        const info: DownloadInfo = await response.json();
        const download = info.downloads.find(value => value.id === installer.id && value.available);
        if (download && isInstallerUrl(download.href)) location = download.href;
      }
    } catch { location = releasesPage; }
  }
  return new Response(null, {
    status: 302,
    headers: { Location: location, 'Cache-Control': !installer ? 'no-store' : location === releasesPage ? fallbackCache : releaseCache },
  });
}
