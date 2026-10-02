import { useId, useState } from 'react';
import { useMeshFlow } from '../hooks/useMeshFlow';

const paths = [
  'M305 218 C445 218 465 134 630 134',
  'M748 220 C748 267 717 279 717 330',
  'M600 405 C404 405 467 292 305 292',
];
const stages = ['A task leaves your laptop.', 'Forge builds it with the team.', 'Loft reviews the result.', 'The result comes home.'];

function Agent({ x, y, name, role, warm = false }: { x: number; y: number; name: string; role: string; warm?: boolean }) {
  return <g transform={`translate(${x} ${y})`}>
    <rect width="31" height="31" rx="12" className={warm ? 'agent-seat warm' : 'agent-seat'} />
    <path d="M10 11v9m6-12v12m6-9v9" className="agent-mark" />
    <text x="42" y="12" className="agent-name">{name}</text>
    <text x="42" y="28" className="agent-role">{role}</text>
  </g>;
}

function Home({ x, y, name, machine, number, children, active }: {
  x: number; y: number; name: string; machine: string; number: string; children: React.ReactNode; active: boolean;
}) {
  return <g transform={`translate(${x} ${y})`} className="mesh-home" data-active={active}>
    <rect className="basin-rim" x="-7" y="-7" width="249" height="189" rx="48" />
    <rect className="basin" width="235" height="175" rx="40" />
    <path d="M24 66H211" className="basin-divider" />
    <text x="25" y="30" className="home-name">{name}</text>
    <text x="25" y="49" className="home-machine">{machine}</text>
    <text x="208" y="34" textAnchor="end" className="home-number">{number}</text>
    {children}
  </g>;
}

export function InhabitedMesh({ reducedMotion, paused = false }: { reducedMotion: boolean; paused?: boolean }) {
  const [replay, setReplay] = useState(0);
  const { ref, phase } = useMeshFlow(reducedMotion, paused, replay);
  const id = useId();
  return <div className="inhabited-mesh">
    <div className="mesh-caption"><span className="small-label">FIELD 01 / THREE HOMES, ONE MESH</span><span className="mesh-key"><i /> A Domios home on each machine</span></div>
    <svg ref={ref} viewBox="0 0 960 545" role="img" aria-labelledby={`${id}-title ${id}-desc`}>
      <title id={`${id}-title`}>Work travels between three inhabited Domios homes</title>
      <desc id={`${id}-desc`}>Your laptop sends a build to Forge, a workstation with two agents. Forge sends the result to Loft, a Linux server, for review. The reviewed result returns to your laptop.</desc>
      <g aria-hidden="true">
        <path className="field-boundary" d="M30 101V61H71 M899 483h31v-40" />
        <text className="map-note" x="32" y="84">YOUR MACHINES</text>
        {paths.map((path, index) => <g key={path}><path d={path} className="channel-bank" /><path d={path} className="channel-water" data-route={index} /></g>)}
        <text x="428" y="141" className="route-label">01 / BUILD →</text>
        <text x="769" y="279" className="route-label">02 / REVIEW</text>
        <text x="389" y="442" className="route-label">← 03 / RESULT</text>
        <Home x={70} y={160} name="Here" machine="YOUR LAPTOP · MACOS" number="01" active={phase === 0 || phase === 3}>
          <Agent x={25} y={84} name="Ada" role="Coordinates the work" />
          <text x="25" y="150" className="home-status">{phase === 3 ? '✓ Reviewed result received' : 'A place to begin. And return.'}</text>
        </Home>
        <Home x={630} y={45} name="Forge" machine="WORKSTATION · WINDOWS" number="02" active={phase === 1}>
          <Agent x={25} y={83} name="Lin" role="Builds the idea" />
          <Agent x={25} y={127} name="Moss" role="Checks the details" warm />
        </Home>
        <Home x={600} y={330} name="Loft" machine="HOME SERVER · LINUX" number="03" active={phase === 2}>
          <Agent x={25} y={84} name="Rue" role="Reviews the result" warm />
          <text x="25" y="150" className="home-status">A different machine. Same team.</text>
        </Home>
        {['Build', 'Review', 'Result'].map((label, index) => <g key={label} data-parcel={index} className={`work-parcel parcel-${index}`} style={{ opacity: 0 }}>
          <g data-material><rect x="-35" y="-17" width="70" height="34" rx="17" /><rect x="-45" y="-5" width="20" height="10" rx="5" /></g>
          <text textAnchor="middle" y="4">{label}</text>
        </g>)}
        <text x="71" y="485" className="map-note">MANY ADDRESSES.</text>
        <text x="71" y="507" className="map-footnote">One place for the work.</text>
      </g>
    </svg>
    <div className="mesh-ledger"><span><b>0{phase + 1}</b><span>{stages[phase]}</span></span><button className="text-button" onClick={() => setReplay(value => value + 1)} disabled={reducedMotion}>Replay the journey <span aria-hidden="true">↺</span></button></div>
  </div>;
}
