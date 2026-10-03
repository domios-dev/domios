import { spring, stepSpring } from './hero/physics';

export type StoryNode = 'team' | 'home' | 'memory' | 'engine' | 'telegram';
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

type Leg = { from: StoryNode; to: StoryNode };

const LEGS: Leg[] = [
  { from: 'team', to: 'home' },
  { from: 'home', to: 'memory' },
  { from: 'memory', to: 'home' },
  { from: 'home', to: 'team' },
  { from: 'team', to: 'home' },
  { from: 'home', to: 'engine' },
  { from: 'engine', to: 'home' },
  { from: 'home', to: 'telegram' },
  { from: 'telegram', to: 'home' },
  { from: 'home', to: 'team' },
];

const MAX_STEP = 1 / 30;
const REST_POSITION = 0.001;
const REST_VELOCITY = 0.01;

export function createStory(): StoryState {
  return { phase: 0, ...LEGS[0], progress: 0, velocity: 0.35, stage: 'travel', landing: 0, landingVelocity: 0, cycles: 0 };
}

function advanceLeg(state: StoryState): StoryState {
  const phase = (state.phase + 1) % LEGS.length;
  return {
    phase,
    ...LEGS[phase],
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
