import type { CSSProperties, ReactNode } from "react";
import { easeInOut, easeOut, lerp, range, useStage, type Scene } from "../../engine";

const DURATION = 6;

type Beat = { words: string[]; start: number; end: number };
const BEATS: Beat[] = [
  { words: ["One", "memory"], start: 0, end: 1.9 },
  { words: ["Every", "machine"], start: 1.9, end: 3.9 },
  { words: ["Your", "agents,", "at", "work"], start: 3.9, end: DURATION },
];

const CHAR_EM = 0.56;
const charCount = (beat: Beat) => beat.words.join(" ").length;

/** Each glyph as its own box so motion can address it; spaces keep their width. */
function Glyphs({ text, style }: { text: string; style: (i: number, ch: string) => CSSProperties }) {
  return (
    <>
      {Array.from(text).map((ch, i) => (
        <span key={i} style={{ display: "inline-block", whiteSpace: "pre", ...style(i, ch) }}>
          {ch}
        </span>
      ))}
    </>
  );
}

function Dot({ size, pulse = 0, style }: { size: number; pulse?: number; style?: CSSProperties }) {
  return (
    <span style={{ position: "relative", display: "inline-block", width: size, height: size, marginLeft: size * 0.18, borderRadius: size, background: "var(--ct-accent)", verticalAlign: "baseline", ...style }}>
      {pulse > 0 && <span style={{ position: "absolute", inset: -size * 1.6 * pulse, borderRadius: "50%", border: `2px solid var(--ct-accent)`, opacity: 1 - pulse }} />}
    </span>
  );
}

function Line({ children, portrait, size }: { children: ReactNode; portrait: boolean; size: number }) {
  return (
    <div style={{ display: "flex", flexDirection: portrait ? "column" : "row", alignItems: portrait ? "flex-start" : "baseline", justifyContent: "center", gap: portrait ? 0 : size * 0.28, fontSize: size, fontWeight: 700, letterSpacing: "-0.035em", lineHeight: portrait ? 1.02 : 1, color: "var(--ct-fg)" }}>
      {children}
    </div>
  );
}

/** Beat one builds glyph by glyph and then folds into its own full stop. */
function BeatOne({ t, portrait, size }: { t: number; portrait: boolean; size: number }) {
  const beat = BEATS[0];
  const fold = range(t, 1.45, 1.85, easeInOut);
  const n = charCount(beat);
  let index = -1;
  return (
    <Line portrait={portrait} size={size}>
      {beat.words.map((word, w) => (
        <span key={word} style={{ display: "inline-block", overflow: "hidden", paddingBottom: "0.12em" }}>
          <Glyphs
            text={word}
            style={() => {
              index++;
              const k = range(t, 0.15 + index * 0.05, 0.65 + index * 0.05, easeOut);
              const toEnd = (n - index) * CHAR_EM;
              return { opacity: k * (1 - fold), transform: `translate(${fold * toEnd * (portrait ? 0.3 : 1)}em, ${(1 - k) * 0.9 + fold * (portrait ? (1 - w) * 0.8 : 0)}em) scale(${1 - fold})` };
            }}
          />
          {w === beat.words.length - 1 && <Dot size={size * 0.2} pulse={range(t, 1.85, 1.9)} style={{ transform: `scale(${1 + fold * 0.8})` }} />}
        </span>
      ))}
    </Line>
  );
}

const TWO_GLYPHS = BEATS[1].words.join("").length;

/** Beat two is pulled out of the dot glyph by glyph, then splits along its middle. */
function BeatTwo({ t, portrait, size }: { t: number; portrait: boolean; size: number }) {
  const beat = BEATS[1];
  const split = range(t, 3.45, 3.9, easeInOut);
  const half = (top: boolean) => {
    let index = -1;
    return (
      <div style={{ position: top ? "relative" : "absolute", inset: top ? undefined : 0, clipPath: top ? "inset(0 0 52% 0)" : "inset(48% 0 0 0)", transform: `translateX(${(top ? -1 : 1) * split * size * 1.4}px)`, opacity: 1 - split }}>
        <Line portrait={portrait} size={size}>
          {beat.words.map((word, w) => (
            <span key={word} style={{ display: "inline-block", paddingBottom: "0.12em" }}>
              <Glyphs
                text={word}
                style={(j) => {
                  index++;
                  const at = 1.95 + (TWO_GLYPHS - 1 - index) * 0.04;
                  const k = range(t, at, at + 0.5, easeOut);
                  return { opacity: k, transform: `translateX(${(1 - k) * (word.length - j) * CHAR_EM}em) scale(${lerp(0.1, 1, k)})`, transformOrigin: "100% 70%" };
                }}
              />
              {w === beat.words.length - 1 && <Dot size={size * 0.2} />}
            </span>
          ))}
        </Line>
      </div>
    );
  };
  return (
    <div style={{ position: "relative" }}>
      {half(true)}
      {half(false)}
    </div>
  );
}

/** Beat three drops in word by word and closes on a beating dot. */
function BeatThree({ t, portrait, size }: { t: number; portrait: boolean; size: number }) {
  const beat = BEATS[2];
  const sweep = range(t, 5.0, 5.5, easeOut);
  const beatAt = (at: number) => (t >= at ? range(t, at, at + 0.9) : 0);
  return (
    <Line portrait={portrait} size={size}>
      {beat.words.map((word, w) => {
        const k = range(t, 3.95 + w * 0.16, 4.5 + w * 0.16, easeOut);
        const last = w === beat.words.length - 1;
        return (
          <span key={word} style={{ position: "relative", display: "inline-block", paddingBottom: "0.12em", opacity: k, transform: `translateY(${(1 - k) * -0.6}em) rotate(${(1 - k) * (w % 2 ? 6 : -6)}deg)`, color: w >= 2 ? "var(--ct-fg)" : "var(--ct-muted-fg)" }}>
            {word}
            {last && <Dot size={size * 0.2} pulse={beatAt(5.2) || beatAt(5.75)} />}
            {w >= 2 && <span style={{ position: "absolute", left: 0, bottom: "0.02em", height: size * 0.06, width: `${sweep * 100}%`, borderRadius: size, background: "var(--ct-accent)" }} />}
          </span>
        );
      })}
    </Line>
  );
}

function Kinetic({ t }: { t: number }) {
  const { portrait } = useStage();
  const size = portrait ? 168 : 176;
  const which = BEATS.findIndex((b) => t >= b.start && t < b.end);
  const Beat = [BeatOne, BeatTwo, BeatThree][which < 0 ? BEATS.length - 1 : which];
  const drift = lerp(1, 1.04, range(t, 0, DURATION, (k) => k));
  return (
    <div style={{ position: "absolute", inset: 0, background: "var(--ct-bg)", display: "flex", alignItems: portrait ? "flex-start" : "center", justifyContent: "center", padding: portrait ? "420px 56px 0" : 0, fontFamily: "var(--ct-font-ui)" }}>
      <div style={{ transform: `scale(${drift})`, transformOrigin: portrait ? "50% 0" : "50% 50%" }}>
        <Beat t={t} portrait={portrait} size={size} />
      </div>
    </div>
  );
}

const kinetic: Scene = {
  id: "lab-kinetic",
  duration: DURATION,
  render: (t) => <Kinetic t={t} />,
};

export default kinetic;
