import { BrandMark } from './Icon';
import './mesh.css';

const machines = [
  { id: 'laptop', number: '01', name: 'Laptop', kind: 'YOUR EVERYDAY DESK', agents: ['Atlas', 'Sage'], task: 'Shape the next task' },
  { id: 'workstation', number: '02', name: 'Workstation', kind: 'GPU / LOCAL MODELS', agents: ['Forge', 'Ada'], task: 'Build on another machine' },
  { id: 'server', number: '03', name: 'Home server', kind: 'ALWAYS WITHIN REACH', agents: ['Scout', 'Remy'], task: 'Send the result back' },
  { id: 'cloud', number: '04', name: 'Cloud VM', kind: 'ANOTHER PLACE TO WORK', agents: ['Vale'], task: 'Keep the work connected' },
];

export function AgentNetwork() {
  return <figure className="agent-network" aria-label="An illustration of named agents on a laptop, workstation, home server and cloud VM exchanging tasks and reports through a trusted mesh.">
    <div className="mesh-register" aria-hidden="true"><span>YOUR MACHINES / ONE MESH</span><span>PAIRED & CONNECTED</span></div>
    <div className="mesh-desk" aria-hidden="true">
      <svg className="mesh-routes" viewBox="0 0 1000 340" preserveAspectRatio="none" fill="none">
        <path d="M220 100H780M220 100V250H780V100M500 100V250" />
      </svg>
      <div className="mesh-slip task-slip"><span>↗</span> Task</div>
      <div className="mesh-slip report-slip"><span>↙</span> Report</div>
      {machines.map((machine) => <div className={`machine-home machine-${machine.id}`} key={machine.id}>
        <div className="machine-heading"><BrandMark /><span>domios</span><span className="machine-number">{machine.number}</span></div>
        <div className="machine-name">{machine.name}</div>
        <div className="machine-kind">{machine.kind}</div>
        <div className="machine-agents">{machine.agents.map((agent) => <span key={agent}><i />{agent}</span>)}</div>
        <div className="machine-task">{machine.task}</div>
      </div>)}
    </div>
    <figcaption><span>Different machines.</span> The same team.</figcaption>
  </figure>;
}
