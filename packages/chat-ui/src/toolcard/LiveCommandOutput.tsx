import { useLayoutEffect, useRef } from "react";
import type { ToolEvent } from "@codeterm/chat-core";

export function liveOutputTail(output: string): string {
  output = output.slice(-4097);
  const end = output.endsWith("\n") ? output.length - 1 : output.length;
  const floor = Math.max(0, end - 4096);
  let start = end;
  for (let lines = 0; start > floor && lines < 4; lines += 1) {
    const newline = output.lastIndexOf("\n", start - 1);
    if (newline < floor) return output.slice(floor, end);
    start = newline;
  }
  return output.slice(start > floor ? start + 1 : floor, end);
}

export function LiveCommandOutput({ event }: { event: ToolEvent }) {
  const previous = useRef("");
  const running = (event.commandBatch ?? [event]).find(member =>
    member.kind === "command" && member.status === "pending" && member.message.toolStatus === "running");
  const output = running?.result ? liveOutputTail(running.result) : "";
  useLayoutEffect(() => {
    if (output) previous.current = output;
  }, [output]);
  const tail = output || previous.current;
  if (!tail) return null;
  return <div aria-hidden="true" data-open={Boolean(output)} className="ct-command-live-output">
    <pre data-testid={output ? "chat-live-command-output" : undefined}>{tail}</pre>
  </div>;
}
