import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import {
  createSnake,
  pointKey,
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

function SnakeBoard({ game }: { game: SnakeState }) {
  const segments = new Map(game.snake.map((segment, index) => [pointKey(segment), index === 0 ? "head" : "body"]));
  return (
    <div
      aria-label={`Snake board. Score ${game.score}.`}
      className="snake-board"
      role="img"
      style={{ gridTemplateColumns: `repeat(${SNAKE_SIZE}, minmax(0, 1fr))` }}
    >
      {Array.from({ length: SNAKE_SIZE * SNAKE_SIZE }, (_, index) => {
        const point = { x: index % SNAKE_SIZE, y: Math.floor(index / SNAKE_SIZE) };
        const key = pointKey(point);
        const segment = segments.get(key);
        const food = pointKey(game.food) === key;
        return (
          <div aria-hidden="true" className={`snake-cell${segment ? ` snake-${segment}` : ""}${food ? " snake-food" : ""}`} key={key}>
            {segment === "head" ? (
              <span className={`snake-head-face snake-head-${game.direction}`}>
                <i className="snake-eye snake-eye-a" />
                <i className="snake-eye snake-eye-b" />
                <i className="snake-tongue" />
                <i
                  aria-hidden="true"
                  className="snake-whisker snake-whisker-a"
                  style={{ "--whisker-growth": Math.min(1 + game.snake.length * 0.04, 1.6) } as CSSProperties}
                />
                <i
                  aria-hidden="true"
                  className="snake-whisker snake-whisker-b"
                  style={{ "--whisker-growth": Math.min(1 + game.snake.length * 0.04, 1.6) } as CSSProperties}
                />
              </span>
            ) : null}
            {food ? <span className="snake-food-dot" /> : null}
          </div>
        );
      })}
    </div>
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
