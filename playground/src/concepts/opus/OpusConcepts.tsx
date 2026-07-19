import type { CSSProperties, ReactElement } from 'react';
import './opus-concepts.css';

type Concept = {
  id: string;
  title: string;
  caption: string;
  aria: string;
  stage: ReactElement;
};

const vars = (v: Record<string, string>) => v as CSSProperties;

function series(count: number, base: string, ix: string) {
  return Array.from({ length: count }, (_, i) => (
    <span
      key={i}
      className={`${base} ${ix}${i}`}
      style={vars({ '--i': String(i) })}
      aria-hidden="true"
    />
  ));
}

const CONCEPTS: Concept[] = [
  {
    id: 'relay',
    title: 'Relay',
    caption: 'Task handed off down the agent chain',
    aria: 'A token travels along a chain of four agents, lighting each one as the task is handed off.',
    stage: (
      <div className="opc-relay" aria-hidden="true">
        <span className="opc-relay-track" />
        {series(4, 'opc-node', 'opc-n')}
        <span className="opc-token" />
      </div>
    ),
  },
  {
    id: 'sync',
    title: 'Sync',
    caption: 'Coordination heartbeat across the team',
    aria: 'A central core emits expanding rings, a steady heartbeat that keeps the team in sync.',
    stage: (
      <div className="opc-sync" aria-hidden="true">
        {series(3, 'opc-ring', 'opc-r')}
        <span className="opc-core" />
      </div>
    ),
  },
  {
    id: 'merge',
    title: 'Merge',
    caption: 'Parallel lanes converge into one',
    aria: 'Three parallel lanes each send a change toward a single merge point, which pulses when they land.',
    stage: (
      <div className="opc-merge" aria-hidden="true">
        <span className="opc-lane opc-lane-a" />
        <span className="opc-lane opc-lane-b" />
        <span className="opc-lane opc-lane-c" />
        <span className="opc-chip opc-chip-a" />
        <span className="opc-chip opc-chip-b" />
        <span className="opc-chip opc-chip-c" />
        <span className="opc-merge-target" />
      </div>
    ),
  },
  {
    id: 'standup',
    title: 'Standup',
    caption: 'Turn-taking as each agent reports',
    aria: 'A row of bars where a single raised bar moves across the team, one agent speaking at a time.',
    stage: (
      <div className="opc-standup" aria-hidden="true">
        {series(6, 'opc-bar', 'opc-b')}
      </div>
    ),
  },
  {
    id: 'weave',
    title: 'Weave',
    caption: 'Messages crossing between two rows',
    aria: 'Message dots travel along crossing diagonals between an upper and lower row of agents.',
    stage: (
      <div className="opc-weave" aria-hidden="true">
        {series(3, 'opc-msg', 'opc-m')}
        <span className="opc-peer opc-peer-a" />
        <span className="opc-peer opc-peer-b" />
      </div>
    ),
  },
  {
    id: 'orbit',
    title: 'Orbit',
    caption: 'Workers circling the orchestrator',
    aria: 'Worker dots orbit a central orchestrator core, showing the two-level hierarchy at work.',
    stage: (
      <div className="opc-orbit" aria-hidden="true">
        <span className="opc-orbit-ring" />
        <span className="opc-orbit-spin">
          <span className="opc-worker" style={vars({ '--a': '0deg' })} />
          <span className="opc-worker" style={vars({ '--a': '120deg' })} />
          <span className="opc-worker" style={vars({ '--a': '240deg' })} />
        </span>
        <span className="opc-hub" />
      </div>
    ),
  },
  {
    id: 'pipeline',
    title: 'Pipeline',
    caption: 'A task stepping through the stages',
    aria: 'A task chip advances through queued, running and done stages, lighting each stage as it arrives.',
    stage: (
      <div className="opc-pipeline" aria-hidden="true">
        {series(3, 'opc-slot', 'opc-s')}
        <span className="opc-task" />
      </div>
    ),
  },
  {
    id: 'quorum',
    title: 'Quorum',
    caption: 'Votes gathering until consensus',
    aria: 'Vote dots fill one by one until the quorum is reached, then the group confirms with a pulse.',
    stage: (
      <div className="opc-quorum" aria-hidden="true">
        {series(5, 'opc-vote', 'opc-v')}
        <span className="opc-check" />
      </div>
    ),
  },
];

export function OpusConcepts() {
  return (
    <section className="concept-section opus-concepts" aria-labelledby="opus-concepts-heading">
      <div className="section-heading">
        <div>
          <span className="section-index">02</span>
          <h2 id="opus-concepts-heading">Team activity</h2>
        </div>
        <p>Built with Opus · orchestration and collaboration metaphors</p>
      </div>
      <div className="concept-grid">
        {CONCEPTS.map((concept, index) => (
          <article key={concept.id} className="concept-card">
            <header>
              <span>{String(index + 9).padStart(2, '0')}</span>
              <div>
                <h3>{concept.title}</h3>
                <p>{concept.caption}</p>
              </div>
            </header>
            <div className="concept-stage" role="img" aria-label={concept.aria}>
              <div className="mini-team"><b>Team</b><span>active</span></div>
              <div className="concept-motion opc-stage">
                {concept.stage}
              </div>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

export default OpusConcepts;
