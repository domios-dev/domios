import { useLayoutEffect, useRef, useState } from "react";
import { MessageBubble } from "@codeterm/chat-ui";
import { CHAT_CARDS, CHAT_REPLY } from "../../fixtures/chat";
import { reportAt } from "./chatTimes";
import { Camera, Reveal, useStage, type Key, type Scene } from "../../engine";

const noop = () => undefined;
const CARDS = [...CHAT_CARDS, CHAT_REPLY];
const arrival = reportAt;
// Camera y per card, measured from the rendered column: each rest frame keeps the newest card in view and starts
// in a gap between cards, so the frame edge never cuts a visible card.
const REST = [206, 264, 321, 381, 553, 668, 668];
const keys: Key[] = [
  { t: 0, x: 960, y: REST[0], scale: 1.97 },
  ...REST.slice(1).flatMap((y, index): Key[] => [{ t: arrival(index + 1) - 0.24, y: REST[index] }, { t: arrival(index + 1) - 0.05, y }]),
  { t: 5, y: REST[REST.length - 1], scale: 1.99 },
];

const TOP = 140;
const LANDSCAPE_COLUMN = { left: 510, width: 900 };
// The tall frame narrows the column so the cards reflow and can be shown close enough to read on a phone.
const PORTRAIT_COLUMN = { left: 650, width: 620 };
const PORTRAIT_SCALE = 1.62;
const PORTRAIT_VIEW = 1920 / PORTRAIT_SCALE;

/** Follows the newest card down the reflowed column, from where its cards actually landed. */
function portraitKeys(bottoms: number[] | null): Key[] {
  const x = PORTRAIT_COLUMN.left + PORTRAIT_COLUMN.width / 2;
  const first = TOP + PORTRAIT_VIEW / 2 - 40;
  if (!bottoms) return [{ t: 0, x, y: first, scale: PORTRAIT_SCALE }];
  // A column that fits the frame whole sits centred on its finished height; a taller one follows its newest card.
  const total = bottoms[bottoms.length - 1];
  const rest = total + 80 < PORTRAIT_VIEW ? bottoms.map(() => TOP + total / 2) : bottoms.map((bottom) => Math.max(first, TOP + bottom + 40 - PORTRAIT_VIEW / 2));
  return [
    { t: 0, x, y: rest[0], scale: PORTRAIT_SCALE },
    ...rest.slice(1).flatMap((y, index): Key[] => [{ t: arrival(index + 1) - 0.24, y: rest[index] }, { t: arrival(index + 1) - 0.05, y }]),
  ];
}

function ChatAutomation({ t }: { t: number }) {
  const portrait = useStage().portrait;
  const column = useRef<HTMLDivElement>(null);
  const [bottoms, setBottoms] = useState<number[] | null>(null);
  useLayoutEffect(() => {
    if (!portrait || !column.current) return;
    setBottoms([...column.current.children].map((card) => (card as HTMLElement).offsetTop + (card as HTMLElement).offsetHeight));
  }, [portrait]);
  const { left, width } = portrait ? PORTRAIT_COLUMN : LANDSCAPE_COLUMN;
  return (
    <Camera t={t} width={1920} height={portrait ? 2400 : 1080} keys={portrait ? portraitKeys(bottoms) : keys}>
      <div ref={column} className="gallery-transcript" style={{ position: "absolute", left, top: TOP, width, display: "flex", flexDirection: "column", gap: 14 }}>
        {CARDS.map(({ message, origin }, index) => (
          <Reveal key={message.id} t={t} at={arrival(index)} dur={0.35} dy={22}>
            <MessageBubble message={message} origin={origin} onOriginClick={noop} onOpenJobs={noop} onStopReminder={noop} />
          </Reveal>
        ))}
      </div>
    </Camera>
  );
}

const chatAutomation: Scene = {
  id: "chat-automation",
  duration: 5,
  render: (t) => <ChatAutomation t={t} />,
};

export default chatAutomation;
