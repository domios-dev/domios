import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import type { NormalizedChatMessage } from "@codeterm/chat-core";

export default function SampleTerminal({ cwd, messages }: { cwd: string | null; messages: NormalizedChatMessage[] }) {
  const ref = useRef<HTMLDivElement>(null);
  const terminal = useRef<Terminal | null>(null);
  const rendered = useRef("");
  useEffect(() => {
    const host = ref.current;
    if (!host) return;
    const xterm = new Terminal({ disableStdin: true, cursorBlink: false, fontSize: 13, scrollback: 240, convertEol: true });
    terminal.current = xterm;
    const fit = new FitAddon();
    xterm.loadAddon(fit);
    xterm.open(host);
    const paint = () => {
      const style = getComputedStyle(host);
      xterm.options.theme = {
        background: style.getPropertyValue("--ct-bg").trim(),
        foreground: style.getPropertyValue("--ct-fg").trim(),
        cursor: style.getPropertyValue("--ct-accent").trim(),
      };
    };
    paint();
    const scope = host.getRootNode();
    const theme = new MutationObserver(paint);
    if (scope instanceof ShadowRoot) theme.observe(scope.host, { attributes: true });
    const observer = new ResizeObserver(() => { if (host.clientWidth && host.clientHeight) fit.fit(); });
    observer.observe(host);
    return () => { theme.disconnect(); observer.disconnect(); xterm.dispose(); terminal.current = null; rendered.current = ""; };
  }, []);
  useEffect(() => {
    const xterm = terminal.current;
    if (!xterm) return;
    const transcript = [
      `Domios terminal · session transcript\n${cwd ?? "Fieldnote"}\n\n`,
      ...messages.map((message) => message.type === "tool_call"
        ? `$ ${message.toolSummary ?? message.toolName ?? "tool"}\n`
        : `${message.fromLabel ? `[${message.fromLabel}] ` : ""}${message.content}\n\n`),
    ].join("");
    if (transcript === rendered.current) return;
    if (transcript.startsWith(rendered.current)) xterm.write(transcript.slice(rendered.current.length));
    else {
      xterm.reset();
      xterm.write(transcript);
    }
    rendered.current = transcript;
  }, [cwd, messages]);
  return <div className="absolute inset-0 px-3 py-2" role="region" aria-label="Terminal transcript"><div ref={ref} className="h-full w-full" /></div>;
}
