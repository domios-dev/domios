import { sequence, type Scene } from "../../engine";
import { Graph, Recall, Saved, Write } from "./macro";

const shots: Scene[] = [
  { id: "mob-macro-write", duration: 6, render: (t) => <Write t={t} /> },
  { id: "mob-macro-saved", duration: 6, render: (t) => <Saved t={t} /> },
  { id: "mob-macro-graph", duration: 6, render: (t) => <Graph t={t} /> },
  { id: "mob-macro-recall", duration: 6, render: (t) => <Recall t={t} /> },
];

export default [...shots, sequence("mob-macro-reel", shots)];
