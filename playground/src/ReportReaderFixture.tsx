import "./mocks/setup";
import "./mocks/theme";
import "../../src/styles/globals.css";
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { InboundReportCard } from "../../packages/chat-ui/src/codeterm/InboundReportCard";
import { CompactMessageContent } from "../../packages/chat-ui/src/codeterm/CompactMessageContent";
import { CodeBlock } from "../../packages/chat-ui/src/CodeBlock";

const body = `## Result

Audited the assigned issues against the current source. The final row and final paragraph must remain reachable.

| Issue | Action | Reason | Evidence |
| --- | --- | --- | --- |
${Array.from({ length: 24 }, (_, index) => `| #${651 + index} | Reviewed | Verified source behavior and preserved the observable contract for issue ${651 + index}. | opaque-evidence-${index}-${"wide-source-reference-".repeat(4)} |`).join("\n")}

Final report sentinel: all 24 rows are readable.`;

function Fixture() {
  const [expanded, setExpanded] = useState(true);
  const [light, setLight] = useState(false);
  return <main style={{ minHeight: "100dvh", padding: 16, background: light ? "#f8fafc" : "#0d1117", color: light ? "#17212b" : "#e6edf3", "--ct-bg": light ? "#ffffff" : "#0d1117", "--ct-surface": light ? "#f1f5f9" : "#161b22", "--ct-fg": light ? "#17212b" : "#e6edf3", "--ct-muted-fg": light ? "#475569" : "#8b949e", "--ct-border": light ? "#cbd5e1" : "#30363d", "--ct-accent": light ? "#0369a1" : "#58a6ff" } as React.CSSProperties}>
    <button type="button" onClick={() => setLight(!light)}>{light ? "Use dark theme" : "Use light theme"}</button>
    <h1>Chat full-screen reader</h1>
    <InboundReportCard sender="Lens" expanded={expanded} onToggle={() => setExpanded(!expanded)}
      message={{ id: "fixture-report", type: "user", content: `status: in_review\n\n${body}` }} />
    <CompactMessageContent text={Array.from({ length: 30 }, (_, i) => `Long message line ${i + 1}`).join("\n")} />
    <CodeBlock language="typescript" text={Array.from({ length: 40 }, (_, i) => `const value${i} = ${i};`).join("\n")} collapsible />
  </main>;
}

createRoot(document.getElementById("root")!).render(<Fixture />);
