import { Icon } from '../Icon';
import { DownloadControl } from './DownloadControl';
import type { DownloadSelection } from './useDownload';

export function DownloadActions({ download, github, placement = 'hero' }: {
  download: DownloadSelection;
  github: string;
  placement?: 'hero' | 'closing';
}) {
  return <div className="hero-actions">
    <DownloadControl download={download} placement={placement} />
    <a className="button secondary" href={github}><Icon name="github" size={18} /><span>Find us on GitHub</span><Icon name="arrow" size={16} /></a>
  </div>;
}
