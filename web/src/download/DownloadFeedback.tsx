import { useLayoutEffect, useRef } from 'react';
import { installers, releasesPage } from './catalog';
import type { DownloadSelection } from './useDownload';
import './download.css';

function useDownloadDialog(download: DownloadSelection) {
  const ref = useRef<HTMLDialogElement>(null);
  const latest = useRef(download);
  latest.current = download;
  const open = download.feedback.status !== 'idle';
  useLayoutEffect(() => {
    const dialog = ref.current;
    if (!dialog || !open) return;
    dialog.showModal();
    dialog.querySelector<HTMLButtonElement>('[data-download-close]')?.focus();
    return () => {
      if (dialog.open) dialog.close();
      latest.current.restoreFocus();
    };
  }, [open]);
  useLayoutEffect(() => {
    const dialog = ref.current;
    const focused = document.activeElement;
    if (dialog?.open && (!(focused instanceof HTMLElement) || !dialog.contains(focused) || !focused.getClientRects().length)) {
      dialog.querySelector<HTMLButtonElement>('[data-download-close]')?.focus();
    }
  }, [download.feedback.status]);
  return ref;
}

export function DownloadFeedback({ download }: { download: DownloadSelection }) {
  const dialog = useDownloadDialog(download);
  const state = download.feedback;
  const title = state.status === 'starting' ? 'Your download is starting'
    : state.status === 'pending' ? 'Finding your installer'
    : state.status === 'unsupported' ? 'Choose a desktop installer'
    : 'The download could not start';
  return <dialog ref={dialog} className="download-feedback" aria-labelledby="download-title" aria-describedby="download-description"
    data-download-state={state.status} onCancel={event => { event.preventDefault(); download.dismiss(); }}
    onClose={() => { if (!dialog.current?.open) download.dismiss(); }}>
    <div className="download-feedback-top"><span className="eyebrow">DOMIOS / DOWNLOAD</span><button type="button" data-download-close onClick={download.dismiss} aria-label="Close download dialog">×</button></div>
    <div aria-live="polite" aria-atomic="true">
      <h2 id="download-title">{title}</h2>
      {state.installer && <p className="download-platform">{state.installer.label}</p>}
      <p id="download-description">{state.status === 'starting' ? 'Check your browser’s downloads. You may be asked where to save the installer.'
        : state.status === 'pending' ? 'Checking the latest release for your selected platform.'
        : state.status === 'unsupported' ? 'Domios runs on macOS, Windows, and Linux. Pick an installer for your desktop machine.'
        : state.status === 'error' ? state.message : ''}</p>
    </div>
    {state.status === 'starting' && <p className="download-retry">Download did not start? <a data-download-retry href={state.asset.href}>Try again</a></p>}
    {state.status === 'error' && <button className="button primary download-lookup-retry" type="button" onClick={download.retryLookup}>Try again</button>}
    <details className="download-feedback-choices" open={state.status === 'error' || state.status === 'unsupported'}>
      <summary>Other platforms</summary>
      <ul aria-label="Desktop installers">{installers.map(installer => <li key={installer.id}><a
        data-download-trigger="alternative" data-installer-id={installer.id} href={download.hrefFor(installer)}
        onClick={event => download.onClick(event, installer)}>{installer.label}</a></li>)}</ul>
    </details>
    <a className="download-feedback-releases" href={releasesPage}>All releases <span aria-hidden="true">↗</span></a>
  </dialog>;
}
