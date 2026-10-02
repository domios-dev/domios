import { BrandMark, Icon } from './Icon';
import type { IconName } from './Icon';
import { useLiquidMesh } from './hero/useLiquidMesh';
import './mesh.css';

type Agent = { name: string; provider: string; icon: IconName };
const machines: { id: string; number: string; name: string; kind: string; agents: Agent[]; task: string }[] = [
  { id: 'laptop', number: '01', name: 'Laptop', kind: 'YOUR EVERYDAY DESK', agents: [{ name: 'Atlas', provider: 'Claude Code', icon: 'branch' }, { name: 'Sage', provider: 'Codex', icon: 'terminal' }], task: 'Shape the next task' },
  { id: 'workstation', number: '02', name: 'Workstation', kind: 'GPU / LOCAL MODELS', agents: [{ name: 'Forge', provider: 'LM Studio', icon: 'layers' }, { name: 'Ada', provider: 'OpenCode', icon: 'grid' }], task: 'Build on another machine' },
  { id: 'server', number: '03', name: 'Home server', kind: 'ALWAYS WITHIN REACH', agents: [{ name: 'Scout', provider: 'Grok Build', icon: 'mesh' }, { name: 'Remy', provider: 'Codex', icon: 'terminal' }], task: 'Send the result back' },
  { id: 'cloud', number: '04', name: 'Cloud VM', kind: 'ANOTHER PLACE TO WORK', agents: [{ name: 'Vale', provider: 'Claude Code', icon: 'branch' }], task: 'Keep the work connected' },
];

export function AgentNetwork() {
  const desk = useLiquidMesh();
  return <figure className="agent-network" role="img" aria-label="Domios windows on your laptop, workstation, home server and cloud VM. Named Claude Code, Codex, OpenCode, Grok Build and local LM Studio agents exchange tasks and return reports. For example: Build the landing page. Done: 3 files, tests pass.">
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
          <div className="machine-heading"><span className="machine-controls" aria-hidden="true"><i /><i /><i /></span><span className="machine-title"><BrandMark /><span>domios</span></span><span className="machine-number">{machine.number}</span></div>
          <div className="machine-name">{machine.name}</div>
          <div className="machine-kind">{machine.kind}</div>
          <div className="machine-agents">{machine.agents.map((agent) => <span className="machine-agent" key={agent.name}><span className="agent-name"><i />{agent.name}</span><span className="agent-provider"><Icon name={agent.icon} size={12} />{agent.provider}</span></span>)}</div>
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
          <g className="liquid-label"><text textAnchor="middle" dy="3.5" /></g>
        </g>)}
      </svg>
    </div>
  </figure>;
}
