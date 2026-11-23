import { useEffect, useMemo, useState, type ReactNode } from "react";

const BAR = 56;
const clock = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, "0")}`;

export type Word = { text: string; start: number; end: number };

/** The posts' word-pop cards: up to four words, broken at punctuation or a pause, the spoken word in the accent. */
export function cardsOf(words: Word[]): Word[][] {
  const cards: Word[][] = [];
  let cur: Word[] = [];
  for (const w of words) {
    const last = cur[cur.length - 1];
    if (last && (w.start - last.end > 0.5 || cur.length >= 4 || /[.,:;!?…]$/.test(last.text))) {
      cards.push(cur);
      cur = [];
    }
    cur.push(w);
  }
  if (cur.length) cards.push(cur);
  return cards;
}

export function Captions({ cards, t, width, height }: { cards: Word[][]; t: number; width: number; height: number }) {
  const index = cards.findIndex((card, i) => t >= card[0].start - 0.03 && t < Math.min(card[card.length - 1].end + 0.3, cards[i + 1]?.[0].start ?? Infinity));
  if (index < 0) return null;
  const card = cards[index];
  const spoken = card.filter((w) => t >= w.start - 0.03);
  const pop = Math.min(1, (t - card[0].start + 0.03) / 0.09);
  return (
    <div style={{ position: "absolute", left: 0, right: 0, bottom: height * 0.09, display: "flex", justifyContent: "center", pointerEvents: "none" }}>
      <div style={{ fontFamily: '"Segoe UI", system-ui, sans-serif', fontWeight: 700, fontSize: Math.max(width, height) * 0.032, color: "#fff", background: "rgba(11,13,17,0.78)", padding: "0.12em 0.45em", borderRadius: 10, transform: `scale(${0.92 + 0.08 * pop})` }}>
        {spoken.map((w, i) => (
          <span key={i} style={{ color: i === spoken.length - 1 ? "#58a6ff" : "#fff" }}>{i ? " " : ""}{w.text.replace(/[,;:]$/, "")}</span>
        ))}
      </div>
    </div>
  );
}

/** Live playback with a voice-over track: the audio clock drives t, and the bar scrubs it like a video player. */
export function AudioPlayer({ src, captions, light: initialLight, onTheme, duration, width, height, frame }: {
  src: string; captions?: string; light: boolean; onTheme: (light: boolean) => void; duration: number; width: number; height: number; frame: (t: number) => ReactNode;
}) {
  const audio = useMemo(() => new Audio(src), [src]);
  const [light, setLight] = useState(initialLight);
  const flipTheme = () => { onTheme(!light); setLight(!light); };
  const [cards, setCards] = useState<Word[][]>([]);
  useEffect(() => {
    if (!captions) return;
    void fetch(captions).then((r) => r.json()).then((words: Word[]) => setCards(cardsOf(words)));
  }, [captions]);
  const [t, setT] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [view, setView] = useState({ w: innerWidth, h: innerHeight });

  useEffect(() => {
    let raf = 0;
    const tick = () => {
      setT(audio.currentTime);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    const sync = () => setPlaying(!audio.paused);
    const resize = () => setView({ w: innerWidth, h: innerHeight });
    const keys = (e: KeyboardEvent) => {
      if (e.code === "Space") { e.preventDefault(); void (audio.paused ? audio.play() : audio.pause()); }
      if (e.code === "ArrowRight") audio.currentTime = Math.min(duration, audio.currentTime + 5);
      if (e.code === "ArrowLeft") audio.currentTime = Math.max(0, audio.currentTime - 5);
    };
    audio.addEventListener("play", sync);
    audio.addEventListener("pause", sync);
    audio.addEventListener("ended", sync);
    addEventListener("resize", resize);
    addEventListener("keydown", keys);
    return () => {
      cancelAnimationFrame(raf);
      audio.pause();
      audio.removeEventListener("play", sync);
      audio.removeEventListener("pause", sync);
      audio.removeEventListener("ended", sync);
      removeEventListener("resize", resize);
      removeEventListener("keydown", keys);
    };
  }, [audio, duration]);

  const scale = Math.min(view.w / width, (view.h - BAR) / height);
  const toggle = () => void (audio.paused ? audio.play() : audio.pause());

  return (
    <div style={{ position: "fixed", inset: 0, background: "#000", display: "flex", flexDirection: "column" }}>
      <div style={{ flex: 1, display: "grid", placeItems: "center", overflow: "hidden" }} onClick={toggle}>
        <div style={{ width: width * scale, height: height * scale }}>
          <div style={{ position: "relative", width, height, transform: `scale(${scale})`, transformOrigin: "0 0" }}>
            {/* inert: product parts inside, such as Point and Ask's autofocused field, must never take focus and scroll the picture. */}
            <div inert style={{ position: "absolute", inset: 0 }}>{frame(Math.min(t, duration - 1e-3))}</div>
            <Captions cards={cards} t={t} width={width} height={height} />
          </div>
        </div>
      </div>
      <div style={{ height: BAR, display: "flex", alignItems: "center", gap: 14, padding: "0 18px", background: "#111", color: "#ddd", fontFamily: "system-ui, sans-serif", fontSize: 14 }}>
        <button type="button" onClick={toggle} aria-label={playing ? "Pause" : "Play"} style={{ width: 36, height: 36, borderRadius: 18, border: "none", background: "#2f81f7", color: "#fff", fontSize: 15, cursor: "pointer" }}>
          {playing ? "❚❚" : "▶"}
        </button>
        <span style={{ fontVariantNumeric: "tabular-nums", minWidth: 92 }}>{clock(t)} / {clock(duration)}</span>
        <input type="range" aria-label="Seek" min={0} max={duration} step={0.01} value={t} onChange={(e) => { audio.currentTime = Number(e.target.value); setT(audio.currentTime); }} style={{ flex: 1, accentColor: "#2f81f7" }} />
        <button type="button" onClick={flipTheme} style={{ height: 32, padding: "0 12px", borderRadius: 16, border: "1px solid #333", background: "transparent", color: "#ddd", cursor: "pointer" }}>
          {light ? "Dark" : "Light"}
        </button>
      </div>
    </div>
  );
}
