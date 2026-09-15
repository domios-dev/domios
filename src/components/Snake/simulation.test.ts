import { describe, expect, it } from "vitest";
import {
  createSnakeGame,
  pauseSnakeGame,
  queueSnakeDirection,
  restartSnakeGame,
  resumeSnakeGame,
  startSnakeGame,
  tickSnakeGame,
  type SnakeCell,
  type SnakeDirection,
  type SnakeGameState,
} from "./simulation";

function cells(state: SnakeGameState): SnakeCell[] {
  return state.snake.map((cell) => ({ x: cell.x, y: cell.y }));
}

function hasFoodOnSnake(state: SnakeGameState): boolean {
  return state.food != null && state.snake.some((cell) => cell.x === state.food?.x && cell.y === state.food?.y);
}

function makeGame(overrides: {
  snake?: SnakeCell[];
  direction?: SnakeDirection;
  food?: SnakeCell | null;
  seed?: number | string;
  score?: number;
  highScore?: number;
} = {}): SnakeGameState {
  return createSnakeGame({
    width: 8,
    height: 8,
    seed: overrides.seed ?? "snake-test",
    initialDirection: overrides.direction ?? "right",
    initialSnake: overrides.snake ?? [
      { x: 3, y: 3 },
      { x: 2, y: 3 },
      { x: 1, y: 3 },
    ],
    initialFood: "food" in overrides ? overrides.food : { x: 7, y: 7 },
    score: overrides.score,
    highScore: overrides.highScore,
  });
}

function traceFor(seed: string): Array<Pick<SnakeGameState, "snake" | "direction" | "food" | "score" | "status">> {
  let state = startSnakeGame(createSnakeGame({ width: 10, height: 10, seed }));
  const trace = [];
  for (const direction of ["up", "left", "down", "right"] satisfies SnakeDirection[]) {
    state = queueSnakeDirection(state, direction);
    state = tickSnakeGame(state);
    trace.push({
      snake: state.snake,
      direction: state.direction,
      food: state.food,
      score: state.score,
      status: state.status,
    });
  }
  return trace;
}

describe("Snake simulation", () => {
  it("initializes deterministically from a seed and never places food on the snake", () => {
    const first = createSnakeGame({ width: 8, height: 8, seed: "repeatable" });
    const second = createSnakeGame({ width: 8, height: 8, seed: "repeatable" });

    expect(first).toEqual(second);
    expect(first.status).toBe("ready");
    expect(hasFoodOnSnake(first)).toBe(false);
  });

  it("applies one queued legal turn per tick and rejects opposite reversals", () => {
    let state = makeGame();
    state = queueSnakeDirection(state, "left");
    expect(state.queuedDirections).toEqual([]);

    state = queueSnakeDirection(state, "up");
    state = queueSnakeDirection(state, "down");
    state = queueSnakeDirection(state, "left");
    expect(state.queuedDirections).toEqual(["up", "left"]);

    state = tickSnakeGame(startSnakeGame(state));
    expect(state.direction).toBe("up");
    expect(state.queuedDirections).toEqual(["left"]);
    expect(cells(state)).toEqual([
      { x: 3, y: 2 },
      { x: 3, y: 3 },
      { x: 2, y: 3 },
    ]);

    state = tickSnakeGame(state);
    expect(state.direction).toBe("left");
    expect(state.queuedDirections).toEqual([]);
    expect(cells(state)).toEqual([
      { x: 2, y: 2 },
      { x: 3, y: 2 },
      { x: 3, y: 3 },
    ]);
  });

  it("produces identical state traces for identical seeds and input steps", () => {
    expect(traceFor("same-seed")).toEqual(traceFor("same-seed"));
  });

  it("consumes food, grows, scores, updates high score, and respawns food off the snake", () => {
    let state = makeGame({
      snake: [
        { x: 2, y: 2 },
        { x: 1, y: 2 },
        { x: 0, y: 2 },
      ],
      food: { x: 3, y: 2 },
      seed: 9,
    });

    state = tickSnakeGame(startSnakeGame(state));

    expect(state.score).toBe(1);
    expect(state.highScore).toBe(1);
    expect(cells(state)).toEqual([
      { x: 3, y: 2 },
      { x: 2, y: 2 },
      { x: 1, y: 2 },
      { x: 0, y: 2 },
    ]);
    expect(state.food).not.toBeNull();
    expect(hasFoodOnSnake(state)).toBe(false);

    const withExistingScore = makeGame({ score: 2, highScore: 5, food: null });
    expect(withExistingScore.score).toBe(2);
    expect(withExistingScore.highScore).toBe(5);
  });

  it("enters wall collision explicitly without moving the snake", () => {
    const before = startSnakeGame(
      makeGame({
        snake: [
          { x: 2, y: 0 },
          { x: 2, y: 1 },
          { x: 2, y: 2 },
        ],
        direction: "up",
      }),
    );

    const after = tickSnakeGame(before);

    expect(after.status).toBe("game_over");
    expect(after.collision).toEqual({ kind: "wall", at: { x: 2, y: -1 } });
    expect(cells(after)).toEqual(cells(before));
  });

  it("detects self collision while allowing the old tail cell to vacate", () => {
    const tailEntry = tickSnakeGame(
      startSnakeGame(
        makeGame({
          snake: [
            { x: 2, y: 2 },
            { x: 2, y: 3 },
            { x: 1, y: 3 },
            { x: 1, y: 2 },
          ],
          direction: "left",
        }),
      ),
    );

    expect(tailEntry.status).toBe("running");
    expect(tailEntry.collision).toBeNull();
    expect(cells(tailEntry)).toEqual([
      { x: 1, y: 2 },
      { x: 2, y: 2 },
      { x: 2, y: 3 },
      { x: 1, y: 3 },
    ]);

    const crash = tickSnakeGame(
      startSnakeGame(
        makeGame({
          snake: [
            { x: 2, y: 2 },
            { x: 2, y: 3 },
            { x: 1, y: 3 },
            { x: 1, y: 2 },
            { x: 1, y: 1 },
            { x: 2, y: 1 },
            { x: 3, y: 1 },
            { x: 3, y: 2 },
          ],
          direction: "up",
        }),
      ),
    );

    expect(crash.status).toBe("game_over");
    expect(crash.collision).toEqual({ kind: "self", at: { x: 2, y: 1 } });
  });

  it("freezes ticks while paused and resumes with the queued direction intact", () => {
    let state = queueSnakeDirection(startSnakeGame(makeGame()), "up");
    const paused = pauseSnakeGame(state);
    const afterPausedTick = tickSnakeGame(paused);

    expect(afterPausedTick).toBe(paused);
    expect(afterPausedTick.queuedDirections).toEqual(["up"]);
    expect(cells(afterPausedTick)).toEqual(cells(state));

    state = tickSnakeGame(resumeSnakeGame(afterPausedTick));
    expect(state.status).toBe("running");
    expect(state.direction).toBe("up");
    expect(cells(state)[0]).toEqual({ x: 3, y: 2 });
  });

  it("restarts to a fresh ready state with no stale queue or collision", () => {
    const base = makeGame({
      snake: [
        { x: 2, y: 0 },
        { x: 2, y: 1 },
        { x: 2, y: 2 },
      ],
      direction: "up",
      highScore: 4,
    });
    const crashed = tickSnakeGame(startSnakeGame(base));
    const restartedAfterCrash = restartSnakeGame(crashed);
    const restartedAfterQueue = restartSnakeGame(queueSnakeDirection(startSnakeGame(base), "left"), 6);

    expect(crashed.status).toBe("game_over");
    expect(restartedAfterCrash.status).toBe("ready");
    expect(restartedAfterCrash.collision).toBeNull();
    expect(restartedAfterCrash.queuedDirections).toEqual([]);
    expect(restartedAfterCrash.score).toBe(0);
    expect(cells(restartedAfterCrash)).toEqual(cells(base));
    expect(restartedAfterCrash.food).toEqual(base.food);
    expect(restartedAfterQueue.status).toBe("ready");
    expect(restartedAfterQueue.queuedDirections).toEqual([]);
    expect(restartedAfterQueue.highScore).toBe(6);
  });
});
