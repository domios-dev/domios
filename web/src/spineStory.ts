import { spring, stepSpring } from './hero/physics';

export type StoryNode = 'team-one' | 'team-two' | 'team-three' | 'home' | 'memory' | 'engine' | 'telegram';
export type StoryStage = 'travel' | 'landing';

export type StoryState = {
  phase: number;
  from: StoryNode;
  to: StoryNode;
  progress: number;
  velocity: number;
  stage: StoryStage;
  landing: number;
  landingVelocity: number;
  cycles: number;
};

type Leg = { from: StoryNode; to: StoryNode; cue: string };

export const STORY_LEGS: readonly Leg[] = [
  { from: 'team-one', to: 'home', cue: 'Recall request' },
  { from: 'home', to: 'memory', cue: 'Read memory' },
  { from: 'memory', to: 'home', cue: 'Return knowledge' },
  { from: 'home', to: 'team-two', cue: 'Share context' },
  { from: 'team-two', to: 'team-one', cue: 'Team handoff' },
  { from: 'team-one', to: 'team-two', cue: 'Work update' },
  { from: 'team-two', to: 'team-three', cue: 'Share findings' },
  { from: 'team-three', to: 'home', cue: 'Needs input' },
  { from: 'home', to: 'engine', cue: 'Ask JEV to observe' },
  { from: 'engine', to: 'home', cue: 'JEV state label' },
  { from: 'home', to: 'telegram', cue: 'Notify with evidence' },
  { from: 'telegram', to: 'home', cue: 'Messenger reply' },
  { from: 'home', to: 'team-three', cue: 'Return to team' },
  { from: 'team-three', to: 'team-two', cue: 'Share the answer' },
  { from: 'team-two', to: 'team-one', cue: 'Team response' },
];

export const STORY_ROUTES: readonly { id: string; from: StoryNode; to: StoryNode; kind: string }[] = [
  { id: 'team-one-home', from: 'team-one', to: 'home', kind: 'team' },
  { id: 'team-two-home', from: 'team-two', to: 'home', kind: 'team' },
  { id: 'team-three-home', from: 'team-three', to: 'home', kind: 'team' },
  { id: 'team-one-two', from: 'team-one', to: 'team-two', kind: 'exchange' },
  { id: 'team-two-three', from: 'team-two', to: 'team-three', kind: 'exchange' },
  { id: 'home-memory', from: 'home', to: 'memory', kind: 'memory' },
  { id: 'home-engine', from: 'home', to: 'engine', kind: 'engine' },
  { id: 'home-telegram', from: 'home', to: 'telegram', kind: 'telegram' },
];

const MAX_STEP = 1 / 30;
const REST_POSITION = 0.001;
const REST_VELOCITY = 0.01;

export function createStory(): StoryState {
  return { phase: 0, from: STORY_LEGS[0].from, to: STORY_LEGS[0].to, progress: 0, velocity: 0.35, stage: 'travel', landing: 0, landingVelocity: 0, cycles: 0 };
}

function advanceLeg(state: StoryState): StoryState {
  const phase = (state.phase + 1) % STORY_LEGS.length;
  return {
    phase,
    from: STORY_LEGS[phase].from,
    to: STORY_LEGS[phase].to,
    progress: 0,
    velocity: 0.35,
    stage: 'travel',
    landing: 0,
    landingVelocity: 0,
    cycles: state.cycles + (phase === 0 ? 1 : 0),
  };
}

export function advanceStory(state: StoryState, elapsed: number): StoryState {
  if (!Number.isFinite(elapsed) || elapsed <= 0) return state;
  const dt = Math.min(elapsed, MAX_STEP);

  if (state.stage === 'travel') {
    const travel = spring(state.progress, state.velocity);
    stepSpring(travel, 1, 32, 5.8, dt);
    if (travel.value >= 1) {
      return { ...state, progress: travel.value, velocity: travel.velocity, stage: 'landing', landing: -0.62, landingVelocity: -1.8 };
    }
    return { ...state, progress: travel.value, velocity: travel.velocity };
  }

  const landing = spring(state.landing, state.landingVelocity);
  stepSpring(landing, 0, 260, 14, dt);
  if (Math.abs(landing.value) < REST_POSITION && Math.abs(landing.velocity) < REST_VELOCITY) {
    return { ...advanceLeg(state), landing: landing.value, landingVelocity: landing.velocity };
  }
  return { ...state, landing: landing.value, landingVelocity: landing.velocity };
}
