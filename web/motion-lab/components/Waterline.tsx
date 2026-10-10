import type { ReactNode } from 'react';
import { useWaterline } from '../hooks/useWaterline';

export function Waterline({ children, reducedMotion = false }: { children: ReactNode; reducedMotion?: boolean }) {
  const { ref, visible } = useWaterline(reducedMotion);
  return <div ref={ref} className="waterline" data-visible={visible}>
    <div className="waterline-seam" aria-hidden="true"><i /><i /><i /></div>
    <div className="waterline-content">{children}</div>
  </div>;
}

export function WaterlineStudy({ reducedMotion }: { reducedMotion: boolean }) {
  return <div className="waterline-study">
    <div className="outgoing-section"><span className="small-label">01 / A PLACE TO START</span><p>Every agent has a home.</p><span className="outgoing-line" /></div>
    <Waterline reducedMotion={reducedMotion}>
      <div className="section-intro"><span className="small-label">02 / ROOM TO GROW</span><h3>Let the work<br />find its people.</h3><p>Your laptop starts the conversation.<br />The rest of your machines join in.</p></div>
      <div className="section-plots"><article><span>01</span><h4>Set a direction.</h4><p>Give a team a purpose and a plan.</p></article><article><span>02</span><h4>Make room.</h4><p>Agents work in spaces of their own.</p></article><article><span>03</span><h4>Bring it home.</h4><p>The result returns to where you are.</p></article></div>
    </Waterline>
  </div>;
}
