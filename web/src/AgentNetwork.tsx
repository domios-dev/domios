import { BrandMark } from './Icon';
import { useLiquidMesh } from './hero/useLiquidMesh';
import './mesh.css';

const machines = [
  { id: 'laptop', number: '01', name: 'Laptop', kind: 'YOUR EVERYDAY DESK', agents: ['Atlas', 'Sage'], task: 'Shape the next task' },
  { id: 'workstation', number: '02', name: 'Workstation', kind: 'GPU / LOCAL MODELS', agents: ['Forge', 'Ada'], task: 'Build on another machine' },
  { id: 'server', number: '03', name: 'Home server', kind: 'ALWAYS WITHIN REACH', agents: ['Scout', 'Remy'], task: 'Send the result back' },
  { id: 'cloud', number: '04', name: 'Cloud VM', kind: 'ANOTHER PLACE TO WORK', agents: ['Vale'], task: 'Keep the work connected' },
];

export function AgentNetwork() {
  const desk = useLiquidMesh();
  return <figure className="agent-network" aria-label="Named agents on your laptop, workstation, home server and cloud VM exchange tasks and return reports through a trusted mesh.">
    <div className="mesh-register" aria-hidden="true"><span>YOUR MACHINES / ONE MESH</span><span><i />PAIRED & CONNECTED</span></div>
    <div className="mesh-desk" ref={desk} aria-hidden="true">
      <svg className="mesh-routes" fill="none">
        {[0, 1, 2, 3].map((index) => <g className="mesh-channel" key={index}>
          <path className="channel-bed" />
          <path className="channel-thread" />
          <path className="channel-current" />
          <circle className="channel-port" r="3" />
          <circle className="channel-port" r="3" />
        </g>)}
      </svg>
      <div className="mesh-center"><span>Work flows.</span><span>Wherever you are.</span></div>
      {machines.map((machine) => <div className={`machine-home machine-${machine.id}`} key={machine.id}>
        <div className="machine-body">
          <div className="machine-sheen" />
          <div className="machine-heading"><BrandMark /><span>domios</span><span className="machine-number">{machine.number}</span></div>
          <div className="machine-name">{machine.name}</div>
          <div className="machine-kind">{machine.kind}</div>
          <div className="machine-agents">{machine.agents.map((agent) => <span key={agent}><i />{agent}</span>)}</div>
          <div className="machine-task">{machine.task}</div>
        </div>
      </div>)}
      <svg className="mesh-liquid" fill="none">
        {[0, 1, 2, 3].map((index) => <g className="liquid-transfer" key={index} opacity="0">
          <path className="liquid-neck" />
          <circle className="liquid-ripple" r="13" />
          <circle className="liquid-satellite" r="3" />
          <g className="liquid-drop">
            <path d="M-12 0C-12-7-6-11 1-10C9-10 13-5 13 0S9 10 1 10C-6 11-12 7-12 0Z" />
            <ellipse className="liquid-glint" cx="2" cy="-4" rx="5" ry="2" />
          </g>
          <g className="liquid-label"><rect x="-28" y="-10" width="56" height="20" rx="10" /><text textAnchor="middle" dy="3.5">Task</text></g>
        </g>)}
      </svg>
    </div>
    <figcaption><span className="mesh-legend"><i />Tasks out<span className="report-key"><i />Reports back</span></span><span>Different machines. <strong>The same team.</strong></span></figcaption>
  </figure>;
}
