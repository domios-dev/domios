import { useState } from 'react';
import type { ButtonHTMLAttributes } from 'react';

export function GatherButton({ children, className = '', ...props }: ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button {...props} className={`gather-button ${className}`}>
    <span className="gather-material" aria-hidden="true" /><span className="gather-neck" aria-hidden="true" /><span className="gather-bead" aria-hidden="true"><span>↗</span></span>
    <span className="gather-label">{children}</span>
  </button>;
}

export function GatherStudy() {
  const [gathered, setGathered] = useState(false);
  return <div className="gather-study" data-gathered={gathered}>
    <div className="gather-sketch" aria-hidden="true"><span /><span /><span /><i>gather / settle / belong</i></div>
    <span className="small-label">A SMALL ACT OF BELONGING</span>
    <h3>Come a little closer.</h3>
    <p>Even the smallest part<br />finds its way home.</p>
    <GatherButton onClick={() => setGathered(value => !value)} aria-pressed={gathered}>{gathered ? 'You belong here' : 'Make yourself at home'}</GatherButton>
    <div className="gather-receipt" role="status">{gathered ? <span key="gathered">✓ A place to begin.</span> : <span>Hover, focus, or press. A gesture study.</span>}</div>
  </div>;
}
