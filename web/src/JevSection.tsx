import { BrandMark } from './BrandMark';
import { useSpineStory } from './useSpineStory';
import { STORY_ROUTES } from './spineStory';
import './jev.css';

export function JevSection() {
  const board = useSpineStory();

  return <section id="jev" className="jev-section section-wrap" aria-labelledby="jev-title">
    <div className="jev-heading">
      <h2 id="jev-title">A home for your AI teams.</h2>
    </div>
    <figure
      className="jev-board jev-scene"
      ref={board}
      data-story-motion="paused"
      data-story-phase="static"
      role="img"
      aria-label="Three AI teams exchange context and hand off work to each other. Each has Orchestrator, Planner and Worker roles. Domios connects the teams to shared memory on this machine and a connected messenger. JEV provides optional state labels; Domios authorizes actions from evidence."
    >
      <svg className="jev-routes" aria-hidden="true" preserveAspectRatio="none">
        <defs>
          <marker id="jev-route-arrow" markerWidth="8" markerHeight="8" refX="8" refY="4" orient="auto-start-reverse" markerUnits="userSpaceOnUse">
            <path d="M0 0L8 4L0 8Z" />
          </marker>
        </defs>
        {STORY_ROUTES.map(route => <path key={route.id} className={`jev-route jev-route-${route.kind}`} data-story-route={route.id} markerStart="url(#jev-route-arrow)" markerEnd="url(#jev-route-arrow)" />)}
        <ellipse className="jev-signal" data-story-signal cx="0" cy="0" rx="7" ry="4" opacity="0" />
      </svg>
      <div className="jev-teams">
        {(['team-one', 'team-two', 'team-three'] as const).map((name, index) => <div key={name} className="jev-team jev-node" data-story-node={name}>
          <strong>AI team <span className="jev-team-number">0{index + 1}</span></strong>
          <span className="jev-roles">Orchestrator · Planner · Workers</span>
        </div>)}
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
  </section>;
}
