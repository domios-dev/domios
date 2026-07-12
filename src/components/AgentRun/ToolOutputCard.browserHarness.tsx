import { createRoot, type Root } from "react-dom/client";
import { ToolOutputCard } from "./ToolOutputCard";

export function mountToolOutputCardHarness(host: HTMLElement, payload: string): Root {
  const root = createRoot(host);
  root.render(
    <div style={{ minWidth: 0, maxWidth: "100%" }}>
      <ToolOutputCard kind="call" toolName="exec" payload={payload} />
      <ToolOutputCard kind="call" toolName="wait" payload="one line" />
      <ToolOutputCard kind="call" toolName="ssh" payload="ssh success" />
      <ToolOutputCard kind="call" toolName="mesh" payload="mesh timeout" />
      <ToolOutputCard kind="call" toolName="plugin" payload="plugin result" />
      <ToolOutputCard kind="result" payload="success" isError={false} />
      <ToolOutputCard kind="result" payload="failure" isError />
      <ToolOutputCard kind="result" payload="timeout" isError />
    </div>,
  );
  return root;
}
