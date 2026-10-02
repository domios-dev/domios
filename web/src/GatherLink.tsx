import type { AnchorHTMLAttributes } from 'react';
import { Icon } from './Icon';
import './gather.css';

export function GatherLink({ children, className = '', ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) {
  return <a {...props} className={`gather-link ct-liquid-gather ${className}`}>
    <span className="gather-material" aria-hidden="true" />
    <span className="gather-neck" aria-hidden="true" />
    <span className="gather-bead" aria-hidden="true"><Icon name="arrow" size={18} /></span>
    <span className="gather-label">{children}</span>
  </a>;
}
