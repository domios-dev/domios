import { Icon } from '../Icon';
import { downloadHref, installers, releasesPage } from './catalog';
import type { DownloadSelection } from './useDownload';
import './download.css';

export function DownloadControl({ download }: { download: DownloadSelection }) {
  return <div className="download-control">
    <a className="button primary" href={download.href}><span className="download-label"><span>{download.label}</span><small>{download.detail}</small></span><Icon name="download" size={18} /></a>
    <details className="download-alternatives">
      <summary>Other platforms</summary>
      <div className="download-menu"><ul aria-label="Desktop installers">{installers.map(installer => <li key={installer.id}><a href={downloadHref(installer)}>{installer.label}<Icon name="download" size={13} /></a></li>)}</ul><a className="download-releases" href={releasesPage}>All releases <span aria-hidden="true">↗</span></a></div>
    </details>
  </div>;
}
