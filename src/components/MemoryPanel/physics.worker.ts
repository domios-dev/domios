/// <reference lib="webworker" />
import type {
  PhysicsRequest,
  PhysicsResponse,
} from "./physicsProtocol";
import { ENERGY_SLEEP, PhysicsSim } from "./physicsSim";

const TICK_INTERVAL_MS = 16;
const MAX_TICKS_PER_HEAT = 320;
/** Cap the worker's duty cycle: sleep at least as long as the step took. */
const DUTY_FACTOR = 1;

const sim = new PhysicsSim();
let enabled = true;
let sleeping = false;
let timer: ReturnType<typeof setTimeout> | null = null;
let ticksSinceHeat = 0;
let firstSettleDone = false;

function post(msg: PhysicsResponse) {
  (self as unknown as Worker).postMessage(msg);
}

function tick() {
  timer = null;
  if (!enabled) return;
  if (sim.nodeCount === 0) return;
  ticksSinceHeat++;

  const t0 = performance.now();
  const energy = sim.step();
  const stepMs = performance.now() - t0;

  post({ type: "tick", positions: sim.snapshot() });

  const energySleep = energy < ENERGY_SLEEP;
  const wallclockSleep = ticksSinceHeat >= MAX_TICKS_PER_HEAT;
  if (energySleep || wallclockSleep) {
    sim.zeroVelocities();
    sleeping = true;
    firstSettleDone = true;
    stopLoop();
    return;
  }
  // Adaptive cadence (#128): a big graph's step can exceed the frame budget; stretching the interval by the measured step time bounds the
  // worker's CPU share instead of pegging a core at 100%.
  const delay = Math.max(TICK_INTERVAL_MS, stepMs * DUTY_FACTOR);
  timer = setTimeout(tick, delay);
}

function startLoop() {
  if (timer != null) return;
  sleeping = false;
  ticksSinceHeat = 0;
  timer = setTimeout(tick, TICK_INTERVAL_MS);
}

function stopLoop() {
  if (timer != null) {
    clearTimeout(timer);
    timer = null;
  }
}

self.addEventListener("message", (ev: MessageEvent<PhysicsRequest>) => {
  const msg = ev.data;
  switch (msg.type) {
    case "setGraph": {
      const topologyChanged = sim.setGraph(msg.nodes, msg.edges);
      if (enabled && topologyChanged && !firstSettleDone) {
        sleeping = false;
        startLoop();
      }
      break;
    }
    case "updatePosition": {
      sim.setPosition(msg.id, msg.x, msg.y);
      if (enabled && sleeping) {
        sleeping = false;
        startLoop();
      }
      break;
    }
    case "heat":
      if (enabled && sleeping) {
        sleeping = false;
        startLoop();
      }
      break;
    case "setEnabled":
      enabled = msg.enabled;
      if (enabled) {
        sleeping = false;
        startLoop();
      } else {
        stopLoop();
      }
      break;
  }
});
