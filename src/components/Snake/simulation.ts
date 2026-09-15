export type SnakeDirection = "up" | "right" | "down" | "left";
export type SnakeLifecycleState = "ready" | "running" | "paused" | "game_over" | "won";
export type SnakeCollisionKind = "wall" | "self";
export type SnakeSeed = number | string;

export interface SnakeCell {
  readonly x: number;
  readonly y: number;
}

export interface SnakeBoard {
  readonly width: number;
  readonly height: number;
}

export interface SnakeCollision {
  readonly kind: SnakeCollisionKind;
  readonly at: SnakeCell;
}

export interface SnakeGameOptions {
  readonly width?: number;
  readonly height?: number;
  readonly seed?: SnakeSeed;
  readonly initialDirection?: SnakeDirection;
  readonly initialSnake?: readonly SnakeCell[];
  readonly initialFood?: SnakeCell | null;
  readonly score?: number;
  readonly highScore?: number;
}

export interface SnakeGameSetup {
  readonly board: SnakeBoard;
  readonly seed: number;
  readonly randomState: number;
  readonly direction: SnakeDirection;
  readonly snake: readonly SnakeCell[];
  readonly food: SnakeCell | null;
  readonly score: number;
}

export interface SnakeGameState {
  readonly setup: SnakeGameSetup;
  readonly board: SnakeBoard;
  readonly seed: number;
  readonly randomState: number;
  readonly status: SnakeLifecycleState;
  readonly snake: readonly SnakeCell[];
  readonly direction: SnakeDirection;
  readonly queuedDirections: readonly SnakeDirection[];
  readonly food: SnakeCell | null;
  readonly score: number;
  readonly highScore: number;
  readonly collision: SnakeCollision | null;
}

const DEFAULT_BOARD_SIZE = 16;
const DEFAULT_SNAKE_LENGTH = 3;
const DEFAULT_SEED = 1;
const RANDOM_A = 1_664_525;
const RANDOM_C = 1_013_904_223;

const STEPS: Record<SnakeDirection, SnakeCell> = {
  up: { x: 0, y: -1 },
  right: { x: 1, y: 0 },
  down: { x: 0, y: 1 },
  left: { x: -1, y: 0 },
};

const OPPOSITE: Record<SnakeDirection, SnakeDirection> = {
  up: "down",
  right: "left",
  down: "up",
  left: "right",
};

export function createSnakeGame(options: SnakeGameOptions = {}): SnakeGameState {
  const board = validateBoard({
    width: options.width ?? DEFAULT_BOARD_SIZE,
    height: options.height ?? DEFAULT_BOARD_SIZE,
  });
  const direction = options.initialDirection ?? "right";
  const seed = normalizeSeed(options.seed ?? DEFAULT_SEED);
  const snake = validateSnake(
    options.initialSnake ? cloneSnake(options.initialSnake) : defaultSnake(board, direction),
    board,
  );
  const initial = initialFood(board, snake, seed, options.initialFood);
  const score = nonNegativeInteger(options.score ?? 0, "score");
  const setup: SnakeGameSetup = {
    board,
    seed,
    randomState: initial.randomState,
    direction,
    snake,
    food: initial.food,
    score,
  };

  return stateFromSetup(setup, options.highScore ?? 0);
}

export function startSnakeGame(state: SnakeGameState): SnakeGameState {
  if (state.status !== "ready") return state;
  return { ...state, status: "running", collision: null };
}

export function pauseSnakeGame(state: SnakeGameState): SnakeGameState {
  if (state.status !== "running") return state;
  return { ...state, status: "paused" };
}

export function resumeSnakeGame(state: SnakeGameState): SnakeGameState {
  if (state.status !== "paused") return state;
  return { ...state, status: "running" };
}

export function toggleSnakePause(state: SnakeGameState): SnakeGameState {
  if (state.status === "running") return pauseSnakeGame(state);
  if (state.status === "paused") return resumeSnakeGame(state);
  return state;
}

export function restartSnakeGame(state: SnakeGameState, highScore: number = state.highScore): SnakeGameState {
  return stateFromSetup(state.setup, highScore);
}

export function queueSnakeDirection(state: SnakeGameState, direction: SnakeDirection): SnakeGameState {
  if (state.status === "game_over" || state.status === "won") return state;
  const latest =
    state.queuedDirections.length > 0 ? state.queuedDirections[state.queuedDirections.length - 1] : state.direction;
  if (direction === latest || direction === OPPOSITE[latest]) return state;
  return { ...state, queuedDirections: [...state.queuedDirections, direction] };
}

export function tickSnakeGame(state: SnakeGameState): SnakeGameState {
  if (state.status !== "running") return state;

  const { direction, queuedDirections } = consumeDirection(state);
  const head = state.snake[0];
  const nextHead = stepFrom(head, direction);
  const base = { ...state, direction, queuedDirections };

  if (!inBounds(nextHead, state.board)) {
    return { ...base, status: "game_over", collision: { kind: "wall", at: nextHead } };
  }

  const eatsFood = state.food != null && sameCell(nextHead, state.food);
  const bodyBeforeTailMoves = eatsFood ? state.snake : state.snake.slice(0, -1);
  if (containsCell(bodyBeforeTailMoves, nextHead)) {
    return { ...base, status: "game_over", collision: { kind: "self", at: nextHead } };
  }

  const snake = eatsFood ? [nextHead, ...state.snake] : [nextHead, ...state.snake.slice(0, -1)];
  const score = state.score + (eatsFood ? 1 : 0);
  const nextFood = eatsFood ? placeFood(state.board, snake, state.randomState) : null;
  const food = eatsFood ? nextFood!.food : state.food;
  const randomState = eatsFood ? nextFood!.randomState : state.randomState;
  const status: SnakeLifecycleState = eatsFood && food == null ? "won" : "running";

  return {
    ...base,
    status,
    snake,
    food,
    score,
    highScore: Math.max(state.highScore, score),
    randomState,
    collision: null,
  };
}

export function isOppositeDirection(a: SnakeDirection, b: SnakeDirection): boolean {
  return OPPOSITE[a] === b;
}

function stateFromSetup(setup: SnakeGameSetup, highScore: number): SnakeGameState {
  return {
    setup,
    board: setup.board,
    seed: setup.seed,
    randomState: setup.randomState,
    status: "ready",
    snake: cloneSnake(setup.snake),
    direction: setup.direction,
    queuedDirections: [],
    food: setup.food ? cloneCell(setup.food) : null,
    score: setup.score,
    highScore: Math.max(nonNegativeInteger(highScore, "highScore"), setup.score),
    collision: null,
  };
}

function validateBoard(board: SnakeBoard): SnakeBoard {
  if (!Number.isInteger(board.width) || board.width < DEFAULT_SNAKE_LENGTH) {
    throw new Error("Snake board width must be an integer of at least 3");
  }
  if (!Number.isInteger(board.height) || board.height < DEFAULT_SNAKE_LENGTH) {
    throw new Error("Snake board height must be an integer of at least 3");
  }
  return { width: board.width, height: board.height };
}

function validateSnake(snake: readonly SnakeCell[], board: SnakeBoard): readonly SnakeCell[] {
  if (snake.length === 0) throw new Error("Snake must contain at least one cell");
  const seen = new Set<string>();
  for (const cell of snake) {
    if (!integerCell(cell)) throw new Error("Snake cells must use integer coordinates");
    if (!inBounds(cell, board)) throw new Error("Snake must stay inside the board");
    const key = cellKey(cell);
    if (seen.has(key)) throw new Error("Snake cannot contain duplicate cells");
    seen.add(key);
  }
  return cloneSnake(snake);
}

function initialFood(
  board: SnakeBoard,
  snake: readonly SnakeCell[],
  seed: number,
  requestedFood: SnakeCell | null | undefined,
): { food: SnakeCell | null; randomState: number } {
  if (requestedFood === undefined) return placeFood(board, snake, seed);
  if (requestedFood == null) return { food: null, randomState: seed };
  if (!integerCell(requestedFood)) throw new Error("Food cells must use integer coordinates");
  if (!inBounds(requestedFood, board)) throw new Error("Food must stay inside the board");
  if (containsCell(snake, requestedFood)) throw new Error("Food cannot spawn on the snake");
  return { food: cloneCell(requestedFood), randomState: seed };
}

function placeFood(
  board: SnakeBoard,
  snake: readonly SnakeCell[],
  randomState: number,
): { food: SnakeCell | null; randomState: number } {
  const empty: SnakeCell[] = [];
  for (let y = 0; y < board.height; y += 1) {
    for (let x = 0; x < board.width; x += 1) {
      const cell = { x, y };
      if (!containsCell(snake, cell)) empty.push(cell);
    }
  }
  if (empty.length === 0) return { food: null, randomState };
  const next = nextRandom(randomState);
  return { food: empty[next.value % empty.length], randomState: next.state };
}

function consumeDirection(state: SnakeGameState): {
  direction: SnakeDirection;
  queuedDirections: readonly SnakeDirection[];
} {
  const [direction = state.direction, ...queuedDirections] = state.queuedDirections;
  return { direction, queuedDirections };
}

function defaultSnake(board: SnakeBoard, direction: SnakeDirection): readonly SnakeCell[] {
  const head = defaultHead(board, direction);
  const step = STEPS[direction];
  return Array.from({ length: DEFAULT_SNAKE_LENGTH }, (_, index) => ({
    x: head.x - step.x * index,
    y: head.y - step.y * index,
  }));
}

function defaultHead(board: SnakeBoard, direction: SnakeDirection): SnakeCell {
  const centerX = Math.floor(board.width / 2);
  const centerY = Math.floor(board.height / 2);
  if (direction === "right") return { x: Math.max(DEFAULT_SNAKE_LENGTH - 1, centerX), y: centerY };
  if (direction === "left") return { x: Math.min(board.width - DEFAULT_SNAKE_LENGTH, centerX), y: centerY };
  if (direction === "down") return { x: centerX, y: Math.max(DEFAULT_SNAKE_LENGTH - 1, centerY) };
  return { x: centerX, y: Math.min(board.height - DEFAULT_SNAKE_LENGTH, centerY) };
}

function stepFrom(cell: SnakeCell, direction: SnakeDirection): SnakeCell {
  const step = STEPS[direction];
  return { x: cell.x + step.x, y: cell.y + step.y };
}

function nextRandom(randomState: number): { state: number; value: number } {
  const state = (Math.imul(randomState, RANDOM_A) + RANDOM_C) >>> 0;
  return { state, value: state };
}

function normalizeSeed(seed: SnakeSeed): number {
  if (typeof seed === "number") {
    if (!Number.isFinite(seed)) throw new Error("Snake seed must be finite");
    return Math.trunc(seed) >>> 0;
  }
  let hash = 2_166_136_261;
  for (let i = 0; i < seed.length; i += 1) {
    hash ^= seed.charCodeAt(i);
    hash = Math.imul(hash, 16_777_619) >>> 0;
  }
  return hash;
}

function nonNegativeInteger(value: number, name: string): number {
  if (!Number.isInteger(value) || value < 0) throw new Error(`${name} must be a non-negative integer`);
  return value;
}

function cloneSnake(snake: readonly SnakeCell[]): readonly SnakeCell[] {
  return snake.map(cloneCell);
}

function cloneCell(cell: SnakeCell): SnakeCell {
  return { x: cell.x, y: cell.y };
}

function containsCell(cells: readonly SnakeCell[], target: SnakeCell): boolean {
  return cells.some((cell) => sameCell(cell, target));
}

function sameCell(a: SnakeCell, b: SnakeCell): boolean {
  return a.x === b.x && a.y === b.y;
}

function integerCell(cell: SnakeCell): boolean {
  return Number.isInteger(cell.x) && Number.isInteger(cell.y);
}

function inBounds(cell: SnakeCell, board: SnakeBoard): boolean {
  return cell.x >= 0 && cell.x < board.width && cell.y >= 0 && cell.y < board.height;
}

function cellKey(cell: SnakeCell): string {
  return `${cell.x}:${cell.y}`;
}
