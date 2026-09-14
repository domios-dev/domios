export const SNAKE_SIZE = 20;

export type Direction = "up" | "right" | "down" | "left";
export type GameStatus = "ready" | "running" | "paused" | "gameover";

export type Point = { x: number; y: number };

export type SnakeState = {
  snake: Point[];
  food: Point;
  direction: Direction;
  queuedDirection: Direction;
  score: number;
  status: GameStatus;
  seed: number;
};

const vectors: Record<Direction, Point> = {
  up: { x: 0, y: -1 },
  right: { x: 1, y: 0 },
  down: { x: 0, y: 1 },
  left: { x: -1, y: 0 },
};

const opposite: Record<Direction, Direction> = {
  up: "down",
  right: "left",
  down: "up",
  left: "right",
};

const samePoint = (a: Point, b: Point) => a.x === b.x && a.y === b.y;
const occupied = (point: Point, snake: Point[]) => snake.some((segment) => samePoint(segment, point));

function nextFood(snake: Point[], seed: number): { food: Point; seed: number } {
  const next = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
  for (let offset = 0; offset < SNAKE_SIZE * SNAKE_SIZE; offset += 1) {
    const index = (next + offset) % (SNAKE_SIZE * SNAKE_SIZE);
    const food = { x: index % SNAKE_SIZE, y: Math.floor(index / SNAKE_SIZE) };
    if (!occupied(food, snake)) return { food, seed: next };
  }
  return { food: { x: 0, y: 0 }, seed: next };
}

export function createSnake(seed = 23): SnakeState {
  const snake = [{ x: 8, y: 10 }, { x: 7, y: 10 }, { x: 6, y: 10 }];
  const result = nextFood(snake, seed);
  return {
    snake,
    food: result.food,
    direction: "right",
    queuedDirection: "right",
    score: 0,
    status: "ready",
    seed: result.seed,
  };
}

export function turnSnake(state: SnakeState, direction: Direction): SnakeState {
  if (state.status === "gameover" || opposite[state.queuedDirection] === direction) return state;
  return {
    ...state,
    queuedDirection: direction,
    status: state.status === "ready" ? "running" : state.status,
  };
}

export function togglePause(state: SnakeState): SnakeState {
  if (state.status === "ready" || state.status === "paused") return { ...state, status: "running" };
  if (state.status === "running") return { ...state, status: "paused" };
  return state;
}

export function stepSnake(state: SnakeState): SnakeState {
  if (state.status !== "running") return state;
  const direction = state.queuedDirection;
  const vector = vectors[direction];
  const head = state.snake[0];
  const nextHead = { x: head.x + vector.x, y: head.y + vector.y };
  const hitsWall = nextHead.x < 0 || nextHead.x >= SNAKE_SIZE || nextHead.y < 0 || nextHead.y >= SNAKE_SIZE;
  const eats = samePoint(nextHead, state.food);
  const body = eats ? state.snake : state.snake.slice(0, -1);

  if (hitsWall || occupied(nextHead, body)) return { ...state, direction, status: "gameover" };

  const snake = eats ? [nextHead, ...state.snake] : [nextHead, ...state.snake.slice(0, -1)];
  if (!eats) return { ...state, snake, direction };
  const food = nextFood(snake, state.seed);
  return { ...state, snake, direction, food: food.food, seed: food.seed, score: state.score + 1 };
}

export function restartSnake(state: SnakeState): SnakeState {
  return createSnake(state.seed + state.score + 1);
}

export const pointKey = (point: Point) => `${point.x}:${point.y}`;
