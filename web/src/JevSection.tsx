import { BrandMark } from './BrandMark';
import { useSpineStory } from './useSpineStory';
import './jev.css';

export function JevSection() {
  const board = useSpineStory();

  return <section id="jev" className="jev-section section-wrap" aria-labelledby="jev-title">
    <div className="jev-heading">
      <h2 id="jev-title">A home for your whole team.</h2>
      <p>Your team, memory and messages in one place.</p>
    </div>
    <figure
      className="jev-board jev-scene"
      ref={board}
      data-story-motion="paused"
      data-story-phase="static"
      role="img"
      aria-label="The Team’s Orchestrator, Planner and Worker roles ask Domios to recall this machine’s memory. JEV is pluggable and names states; Domios authorizes actions from evidence. A connected Telegram notification and reply return through Domios to the Team."
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
        <ellipse className="jev-signal" data-story-signal cx="0" cy="0" rx="7" ry="4" opacity="0" />
      </svg>
      <div className="jev-team jev-node" data-story-node="team">
        <strong>Team</strong>
        <span className="jev-roles">Orchestrator · Planner · Worker</span>
      </div>
      <div className="jev-core-group">
        <div className="jev-home jev-node" data-story-node="home">
          <BrandMark motion="static" />
          <strong>Domios</strong>
          <span className="jev-story-cue" data-story-cue aria-hidden="true">Recall request</span>
        </div>
        <div className="jev-engine jev-node" data-story-node="engine">
          <strong>JEV</strong>
          <span>Pluggable · state labels</span>
        </div>
      </div>
      <div className="jev-memory jev-node" data-story-node="memory">
        <strong>Memory</strong>
        <span>Shared on this machine</span>
      </div>
      <div className="jev-telegram jev-node" data-story-node="telegram">
        <strong>Your messenger</strong>
        <span>Telegram: updates and replies</span>
      </div>
    </figure>
    <p className="jev-roles-description">The Planner shapes tasks. Workers carry them out. The Orchestrator coordinates and reviews.</p>
    <p className="jev-authority">Optional model help for recall and tab status. Domios checks the evidence.</p>
  </section>;
}
