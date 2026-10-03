import { BrandMark } from './BrandMark';
import { useSpineStory } from './useSpineStory';
import './jev.css';

export function JevSection() {
  const board = useSpineStory();

  return <section id="jev" className="jev-section section-wrap" aria-labelledby="jev-title">
    <div className="jev-heading">
      <h2 id="jev-title">Keep your team connected.</h2>
      <p>Shared knowledge, clear roles, and a way to reach you.</p>
    </div>
    <figure
      className="jev-board jev-scene"
      ref={board}
      data-story-motion="paused"
      data-story-phase="static"
      role="img"
      aria-label="Orchestrator, Planner and Worker roles ask Domios to recall this machine’s durable memory. When a task needs input, JEV labels the state; Domios authorizes a notification from evidence through connected Telegram, and replies return through Domios to the Team. JEV is pluggable, and suitable local models can connect through LM Studio."
    >
      <svg className="jev-routes" aria-hidden="true" preserveAspectRatio="none">
        <defs>
          <marker id="jev-route-arrow" markerWidth="8" markerHeight="8" refX="8" refY="4" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
            <path d="M0 0L8 4L0 8Z" />
          </marker>
        </defs>
        <path className="jev-route jev-route-team" data-story-route="team-home" markerStart="url(#jev-route-arrow)" markerEnd="url(#jev-route-arrow)" />
        <path className="jev-route jev-route-memory" data-story-route="home-memory" markerStart="url(#jev-route-arrow)" markerEnd="url(#jev-route-arrow)" />
        <path className="jev-route jev-route-engine" data-story-route="home-engine" markerStart="url(#jev-route-arrow)" markerEnd="url(#jev-route-arrow)" />
        <path className="jev-route jev-route-telegram" data-story-route="home-telegram" markerStart="url(#jev-route-arrow)" markerEnd="url(#jev-route-arrow)" />
        <ellipse className="jev-signal" data-story-signal cx="0" cy="0" rx="7" ry="4" />
      </svg>
      <div className="jev-team jev-node" data-story-node="team">
        <strong>Team</strong>
        <div className="jev-roles">
          <span>Orchestrator</span>
          <span>Planner</span>
          <span>Worker</span>
        </div>
      </div>
      <div className="jev-core-group">
        <div className="jev-home jev-node" data-story-node="home">
          <BrandMark motion="static" />
          <strong>Domios</strong>
          <span className="jev-home-label">Built-in spine</span>
          <div className="jev-spine" aria-hidden="true"><i /><i /><i /><i /><i /></div>
        </div>
        <div className="jev-engine jev-node" data-story-node="engine">
          <strong>JEV</strong>
          <span>Pluggable</span>
        </div>
      </div>
      <div className="jev-memory jev-node" data-story-node="memory">
        <strong>Memory</strong>
        <span>This machine</span>
        <small>Durable knowledge</small>
      </div>
      <div className="jev-telegram jev-node" data-story-node="telegram">
        <strong>Telegram</strong>
        <span>Connected bridge</span>
        <small>Notifications · replies</small>
      </div>
    </figure>
    <p className="jev-authority">The model names states. Domios authorizes actions from evidence.</p>
  </section>;
}
