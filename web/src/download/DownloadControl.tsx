import { Icon } from '../Icon';
import { installers, releasesPage } from './catalog';
import type { DownloadSelection } from './useDownload';
import './download.css';

export function DownloadControl({ download, placement = 'hero' }: { download: DownloadSelection; placement?: 'hero' | 'closing' }) {
  return <div className="download-control">
    <a className="button primary" data-download-trigger={placement} href={download.href} onClick={event => download.onClick(event)}><span className="download-label"><span>{download.label}</span><small>{download.detail}</small></span><Icon name="download" size={18} /></a>
    <details className="download-alternatives">
      <summary>Other platforms</summary>
      <div className="download-menu"><ul aria-label="Desktop installers">{installers.map(installer => <li key={installer.id}><a data-download-trigger="alternative" data-installer-id={installer.id} href={download.hrefFor(installer)} onClick={event => download.onClick(event, installer)}>{installer.label}<Icon name="download" size={13} /></a></li>)}</ul><a className="download-releases" href={releasesPage}>All releases <span aria-hidden="true">↗</span></a></div>
    </details>
  </div>;
}
