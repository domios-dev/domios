import { useEffect, useState } from 'react';
import { downloadHref, releasesPage } from './catalog';
import type { DownloadInfo, Installer } from './catalog';
import { detectInstaller } from './detect';
import type { ClientHints } from './detect';

let metadata: Promise<DownloadInfo | null> | undefined;

export function useDownload() {
  const [installer, setInstaller] = useState<Installer | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    const hints = (navigator as Navigator & { userAgentData?: ClientHints }).userAgentData;
    const detect = (architecture = '') => detectInstaller(navigator.userAgent, hints?.platform || navigator.platform, architecture,
      hints?.mobile || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1));
    setInstaller(detect());
    if (hints?.getHighEntropyValues) {
      void hints.getHighEntropyValues(['architecture', 'bitness']).then(values => {
        if (active) setInstaller(detect(values.architecture));
      }).catch(() => {});
    }
    metadata ??= fetch('/api/download/info', { signal: AbortSignal.timeout(7000) })
      .then(async response => response.ok ? await response.json() as DownloadInfo : null).catch(() => null);
    void metadata.then(info => { if (active && typeof info?.version === 'string') setVersion(info.version); });
    return () => { active = false; };
  }, []);
  return {
    href: installer ? downloadHref(installer) : releasesPage,
    label: installer ? `Download for ${installer.platform}` : 'Download Domios',
    detail: installer ? `${installer.detail} · ${version ?? 'Latest release'}` : 'Domios is for desktop',
  };
}

export type DownloadSelection = ReturnType<typeof useDownload>;
