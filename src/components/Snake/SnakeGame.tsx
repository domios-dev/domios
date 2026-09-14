import { useCallback, useEffect, useRef, useState } from "react";
import {
  createSnake,
  restartSnake,
  SNAKE_SIZE,
  stepSnake,
  togglePause,
  turnSnake,
  type Direction,
  type SnakeState,
} from "./snakeModel";

const TICK_MS = 145;
const keys: Partial<Record<string, Direction>> = {
  ArrowUp: "up", KeyW: "up", ArrowRight: "right", KeyD: "right",
  ArrowDown: "down", KeyS: "down", ArrowLeft: "left", KeyA: "left",
};
const controls: Array<{ direction: Direction; symbol: string; label: string }> = [
  { direction: "up", symbol: "↑", label: "Move up" },
  { direction: "left", symbol: "←", label: "Move left" },
  { direction: "down", symbol: "↓", label: "Move down" },
  { direction: "right", symbol: "→", label: "Move right" },
];

export function isSnakeRoute(search = typeof window === "undefined" ? "" : window.location.search): boolean {
  return new URLSearchParams(search).get("prototype") === "snake";
}

type Projection = (x: number, y: number, z: number) => { x: number; y: number };

function polygon(context: CanvasRenderingContext2D, points: Array<{ x: number; y: number }>, fill: string, stroke?: string) {
  context.beginPath();
  context.moveTo(points[0].x, points[0].y);
  points.slice(1).forEach((point) => context.lineTo(point.x, point.y));
  context.closePath();
  context.fillStyle = fill;
  context.fill();
  if (stroke) {
    context.strokeStyle = stroke;
    context.stroke();
  }
}

function drawCube(context: CanvasRenderingContext2D, project: Projection, x: number, y: number, height: number, head: boolean) {
  const half = head ? 0.44 : 0.4;
  const top = [
    project(x - half, y - half, height),
    project(x + half, y - half, height),
    project(x + half, y + half, height),
    project(x - half, y + half, height),
  ];
  const base = [
    project(x - half, y - half, 0),
    project(x + half, y - half, 0),
    project(x + half, y + half, 0),
    project(x - half, y + half, 0),
  ];
  const topColor = head ? "#d7fff4" : "#72f4d2";
  const sideColor = head ? "#35b9c9" : "#1e98b2";
  const frontColor = head ? "#2789b9" : "#147697";
  polygon(context, [top[0], top[1], top[2], top[3]], topColor, "#b3ffeb66");
  polygon(context, [top[1], top[2], base[2], base[1]], sideColor);
  polygon(context, [top[2], top[3], base[3], base[2]], frontColor);
  if (head) {
    context.fillStyle = "#071225";
    context.beginPath();
    context.arc(top[1].x - 2, top[1].y + 3, 2.5, 0, Math.PI * 2);
    context.fill();
  }
}

function drawScene(context: CanvasRenderingContext2D, width: number, height: number, game: SnakeState, time: number) {
  context.clearRect(0, 0, width, height);
  const scale = Math.min(width, height) / 24;
  const project: Projection = (x, y, z) => ({
    x: width / 2 + (x - y) * scale * 0.72,
    y: height * 0.57 + (x + y - SNAKE_SIZE) * scale * 0.36 - z * scale * 0.58,
  });
  const corners = [project(0, 0, 0), project(SNAKE_SIZE, 0, 0), project(SNAKE_SIZE, SNAKE_SIZE, 0), project(0, SNAKE_SIZE, 0)];
  polygon(context, corners, "#0a1426", "#7893ff66");

  context.lineWidth = 1;
  for (let index = 0; index <= SNAKE_SIZE; index += 1) {
    polygon(context, [project(index, 0, 0), project(index, SNAKE_SIZE, 0)], "transparent", "#6f90ff18");
    polygon(context, [project(0, index, 0), project(SNAKE_SIZE, index, 0)], "transparent", "#6f90ff18");
  }

  [...game.snake]
    .reverse()
    .forEach((segment, index) => drawCube(context, project, segment.x + 0.5, segment.y + 0.5, index === game.snake.length - 1 ? 0.72 : 0.58, index === game.snake.length - 1));

  const pulse = 0.8 + (Math.sin(time / 240) + 1) * 0.08;
  const food = project(game.food.x + 0.5, game.food.y + 0.5, 0.75 * pulse);
  context.shadowColor = "#ff9f74";
  context.shadowBlur = 18;
  context.fillStyle = "#ff9f74";
  context.beginPath();
  context.arc(food.x, food.y, scale * 0.25 * pulse, 0, Math.PI * 2);
  context.fill();
  context.shadowBlur = 0;

  const head = game.snake[0];
  const direction = { up: { x: 0, y: -1 }, right: { x: 1, y: 0 }, down: { x: 0, y: 1 }, left: { x: -1, y: 0 } }[game.direction];
  const whiskerOrigin = project(head.x + 0.5 + direction.x * 0.42, head.y + 0.5 + direction.y * 0.42, 0.83);
  const side = { x: -direction.y, y: direction.x };
  context.strokeStyle = "#b8fff0";
  context.lineWidth = 1.5;
  [-1, 1].forEach((sign) => {
    const tip = project(head.x + 0.5 + direction.x * 1.2 + side.x * sign * 0.42, head.y + 0.5 + direction.y * 1.2 + side.y * sign * 0.42, 0.84);
    context.beginPath();
    context.moveTo(whiskerOrigin.x, whiskerOrigin.y);
    context.lineTo(tip.x, tip.y);
    context.stroke();
  });
}

function SnakeBoard({ game }: { game: SnakeState }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const latestGame = useRef(game);
  latestGame.current = game;

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;
    let frame = 0;
    let width = 0;
    let height = 0;
    const resize = () => {
      const bounds = canvas.getBoundingClientRect();
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      width = bounds.width;
      height = bounds.height;
      canvas.width = Math.max(1, Math.floor(width * ratio));
      canvas.height = Math.max(1, Math.floor(height * ratio));
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();
    const render = (time: number) => {
      drawScene(context, width, height, latestGame.current, time);
      frame = requestAnimationFrame(render);
    };
    frame = requestAnimationFrame(render);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, []);

  return (
    <canvas
      aria-label={`Snake board. Score ${game.score}.`}
      className="snake-scene-3d"
      role="img"
    ref={canvasRef}
  />
  );
}

export function SnakeGame() {
  const [game, setGame] = useState(createSnake);
  const gameRef = useRef(game);
  const update = useCallback((change: (state: SnakeState) => SnakeState) => {
    setGame((current) => {
      const next = change(current);
      gameRef.current = next;
      return next;
    });
  }, []);
  const turn = useCallback((direction: Direction) => update((state) => turnSnake(state, direction)), [update]);
  const pause = useCallback(() => update(togglePause), [update]);
  const restart = useCallback(() => update(restartSnake), [update]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const direction = keys[event.code];
      if (direction) {
        event.preventDefault();
        turn(direction);
      } else if (event.code === "Space") {
        event.preventDefault();
        pause();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pause, turn]);

  useEffect(() => {
    let frame = 0;
    let last = performance.now();
    let accumulated = 0;
    const animate = (now: number) => {
      if (gameRef.current.status === "running") {
        accumulated += Math.min(now - last, 250);
        if (accumulated >= TICK_MS) {
          accumulated %= TICK_MS;
          update(stepSnake);
        }
      } else {
        accumulated = 0;
      }
      last = now;
      frame = requestAnimationFrame(animate);
    };
    frame = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(frame);
  }, [update]);

  const terminal = game.status === "gameover";
  const action = game.status === "running" ? "Pause" : game.status === "paused" ? "Resume" : "Start";
  return (
    <main className="snake-app">
      <div className="snake-shell">
        <header className="snake-header">
          <div>
            <p className="snake-kicker">CodeTerm / local arcade</p>
            <h1>Neon Snake</h1>
            <p className="snake-intro">A responsive Snake board with a visible head, glowing trail, and touch controls.</p>
          </div>
          <span className="snake-status">● {terminal ? "Run ended" : game.status === "paused" ? "Paused" : "Live run"}</span>
        </header>
        <section className="snake-layout" aria-label="Snake game">
          <div className="snake-board-frame">
            <SnakeBoard game={game} />
            {terminal ? (
              <div className="snake-overlay">
                <span>Signal lost</span>
                <h2>Try the next line</h2>
                <p>Your score: {game.score}</p>
                <button className="snake-primary" onClick={restart} type="button">Restart run</button>
              </div>
            ) : null}
          </div>
          <aside className="snake-sidebar">
            <div className="snake-score"><span>Score</span><strong>{String(game.score).padStart(2, "0")}</strong><small>{game.snake.length} segments</small></div>
            <div className="snake-actions">
              <button className="snake-primary" onClick={pause} type="button">{action}</button>
              <button className="snake-secondary" onClick={restart} type="button">New run</button>
            </div>
            <div className="snake-controls-card">
              <div className="snake-card-title"><span>Controls</span><kbd>WASD</kbd></div>
              <div className="snake-controls" aria-label="Touch controls">
                {controls.map((control) => <button aria-label={control.label} className={`snake-control snake-control-${control.direction}`} key={control.direction} onClick={() => turn(control.direction)} type="button">{control.symbol}</button>)}
              </div>
              <p>Arrow keys or tap the pad. Space pauses safely.</p>
            </div>
            <p aria-live="polite" className="snake-live">{terminal ? `Game over at ${game.score} points.` : `Score ${game.score}.`}</p>
          </aside>
        </section>
        <footer className="snake-footer">Local prototype · 20 × 20 field · reduced-motion friendly</footer>
      </div>
    </main>
  );
}
