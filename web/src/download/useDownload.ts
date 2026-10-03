import { useEffect, useRef, useState } from 'react';
import type { MouseEvent } from 'react';
import { downloadHref, releasesPage } from './catalog';
import type { DownloadInfo, Installer } from './catalog';
import { detectInstaller } from './detect';
import type { ClientHints } from './detect';
import { createDownloadFlow, ordinaryDownloadClick } from './flow';
import type { DownloadFeedbackState } from './flow';

export function useDownload() {
  const [installer, setInstaller] = useState<Installer | null>(null);
  const [info, setInfo] = useState<DownloadInfo | null>(null);
  const [feedback, setFeedback] = useState<DownloadFeedbackState>({ status: 'idle', installer: null });
  const trigger = useRef<HTMLAnchorElement | null>(null);
  const frames = useRef<HTMLIFrameElement[]>([]);
  const flow = useRef<ReturnType<typeof createDownloadFlow> | null>(null);
  if (!flow.current) flow.current = createDownloadFlow({
    change: state => {
      setFeedback(state);
      if (state.status === 'starting') setInfo(state.info);
    },
    initiate: asset => {
      const frame = document.createElement('iframe');
      frame.hidden = true;
      frame.title = 'Installer download';
      frame.src = asset.href;
      document.body.append(frame);
      frames.current.push(frame);
    },
  });
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
    return () => {
      active = false;
      flow.current?.close();
      frames.current.forEach(frame => frame.remove());
      frames.current = [];
    };
  }, []);
  const hrefFor = (selection: Installer) => info?.downloads.find(asset => asset.id === selection.id && asset.available)?.href ?? downloadHref(selection);
  const onClick = (event: MouseEvent<HTMLAnchorElement>, selection = installer) => {
    if (!ordinaryDownloadClick(event)) return;
    event.preventDefault();
    if (!event.currentTarget.closest('dialog')) trigger.current = event.currentTarget;
    void flow.current!.start(selection);
  };
  const dismiss = () => flow.current!.close();
  const restoreFocus = () => {
    if (trigger.current?.isConnected) trigger.current.focus({ preventScroll: true });
  };
  const retryLookup = () => {
    if (feedback.status === 'error') void flow.current!.start(feedback.installer);
  };
  return {
    installer,
    assetHref: installer ? info?.downloads.find(asset => asset.id === installer.id && asset.available)?.href ?? null : null,
    href: installer ? hrefFor(installer) : releasesPage,
    hrefFor, onClick, feedback, dismiss, restoreFocus, retryLookup,
    label: installer ? `Download for ${installer.platform}` : 'Download Domios',
    detail: installer ? `${installer.detail} · ${info?.version ?? 'Latest release'}` : 'Domios is for desktop',
  };
}

export type DownloadSelection = ReturnType<typeof useDownload>;
