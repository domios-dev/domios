import { BrandMark } from './Icon';
import { ProviderLogo } from './ProviderLogo';
import type { ProviderLogoId } from './ProviderLogo';
import { useLiquidMesh } from './hero/useLiquidMesh';
import { useMediaQuery } from './usePreferences';
import './mesh.css';

type Agent = { name: string; provider: string; logo: ProviderLogoId };
const machines: { id: string; number: string; name: string; kind: string; agents: Agent[]; task: string }[] = [
  { id: 'laptop', number: '01', name: 'Laptop', kind: 'YOUR EVERYDAY DESK', agents: [{ name: 'Atlas', provider: 'Claude Code', logo: 'claude' }, { name: 'Sage', provider: 'Codex', logo: 'codex' }], task: 'Shape the next task' },
  { id: 'workstation', number: '02', name: 'Workstation', kind: 'GPU / LOCAL MODELS', agents: [{ name: 'Forge', provider: 'LM Studio', logo: 'lmstudio' }, { name: 'Ada', provider: 'OpenCode', logo: 'opencode' }], task: 'Build on another machine' },
  { id: 'server', number: '03', name: 'Home server', kind: 'ALWAYS WITHIN REACH', agents: [{ name: 'Scout', provider: 'Grok Build', logo: 'grok' }, { name: 'Remy', provider: 'Codex', logo: 'codex' }], task: 'Send the result back' },
  { id: 'cloud', number: '04', name: 'Cloud VM', kind: 'ANOTHER PLACE TO WORK', agents: [{ name: 'Vale', provider: 'Claude Code', logo: 'claude' }], task: 'Keep the work connected' },
];

function MachineHardware({ kind }: { kind: string }) {
  return <div className={`machine-hardware hardware-${kind}`} aria-hidden="true">
    <span className="device-frame" />
    {kind === 'laptop' && <><span className="device-camera" /><span className="laptop-deck"><i /></span></>}
    {kind === 'workstation' && <><span className="monitor-stand" /><span className="workstation-tower"><i /><i /><i /></span></>}
    {kind === 'server' && <><span className="server-bays"><i /><i /><i /></span><span className="server-feet" /></>}
    {kind === 'cloud' && <><span className="cloud-layer" /><svg className="cloud-symbol" viewBox="0 0 64 36" fill="none"><path d="M17 29h32a10 10 0 0 0 1-20 16 16 0 0 0-29-1 11 11 0 0 0-4 21Z" /><path d="M25 21h14M29 16h6" /></svg></>}
  </div>;
}

export function AgentNetwork() {
  const desk = useLiquidMesh();
  const mobile = useMediaQuery('(max-width: 650px)');
  const description = mobile
    ? 'Domios windows on three separate computers: your laptop, workstation and home server. Named Claude Code, Codex, OpenCode, Grok Build and local LM Studio agents exchange tasks and return reports.'
    : 'Domios windows on four separate computers: your laptop, workstation, home server and cloud VM. Named Claude Code, Codex, OpenCode, Grok Build and local LM Studio agents exchange tasks and return reports.';
  return <figure className="agent-network" role="img" aria-label={description}>
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
        <MachineHardware kind={machine.id} />
        <div className="machine-body">
          <div className="machine-sheen" />
          <div className="machine-heading"><span className="machine-controls" aria-hidden="true"><i /><i /><i /></span><span className="machine-title"><BrandMark /><span>domios</span></span><span className="machine-number">{machine.number}</span></div>
          <div className="machine-name">{machine.name}</div>
          <div className="machine-kind">{machine.kind}</div>
          <div className="machine-agents">{machine.agents.map((agent) => <span className="machine-agent" key={agent.name}><span className="agent-name"><i />{agent.name}</span><span className="agent-provider"><span className="agent-logo agent-logo-light"><ProviderLogo mark={agent.logo} theme="light" /></span><span className="agent-logo agent-logo-dark"><ProviderLogo mark={agent.logo} theme="dark" /></span><span className="agent-provider-name">{agent.provider}</span></span></span>)}</div>
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
  </figure>;
}
