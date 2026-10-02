import { Icon, BrandMark } from './Icon';

export function AgentNetwork() {
  return <div className="agent-network" aria-hidden="true">
    <div className="network-grid" />
    <svg className="network-lines" viewBox="0 0 1060 230" fill="none" preserveAspectRatio="none">
      <path d="M170 65H330Q380 65 380 115H530M170 175H330Q380 175 380 115M530 115H680Q730 115 730 60H880M680 115Q730 115 730 175H880" />
      <path className="signal" d="M170 65H330Q380 65 380 115H680Q730 115 730 60H880" />
      <path className="signal signal-second" d="M170 175H330Q380 175 380 115H680Q730 115 730 175H880" />
    </svg>
    <div className="agent-node node-one"><span className="agent-avatar violet"><Icon name="terminal" /></span><span><strong>Planner</strong><small>Shaping the work</small></span><i /></div>
    <div className="agent-node node-two"><span className="agent-avatar blue"><Icon name="grid" /></span><span><strong>Orchestrator</strong><small>Keeping it moving</small></span><i /></div>
    <div className="network-core"><BrandMark /><span>DOMIOS</span></div>
    <div className="agent-node node-three"><span className="agent-avatar green"><Icon name="branch" /></span><span><strong>Worker</strong><small>Building in a lane</small></span><i /></div>
    <div className="agent-node node-four"><span className="agent-avatar blue"><Icon name="mesh" /></span><span><strong>Remote agent</strong><small>Connected through mesh</small></span><i /></div>
    <span className="network-note">ONE HOME. MANY POSSIBILITIES.</span>
  </div>;
}
