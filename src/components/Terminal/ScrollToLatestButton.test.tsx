import assert from "node:assert/strict";
import { renderToStaticMarkup } from "react-dom/server";
import { ScrollToLatestButton } from "./ScrollToLatestButton";
import { TerminalViewControls } from "./TerminalViewControls";

const noop = () => {};

const sharedButton = renderToStaticMarkup(
  <ScrollToLatestButton testId="scroll-control" onClick={noop} />,
);
assert(sharedButton.replaceAll(" ", "").includes("color-mix(insrgb,var(--ct-surface)80%,var(--ct-fg)20%)"));
assert.match(sharedButton, /aria-label="Scroll to latest"/);

const fullTerminal = renderToStaticMarkup(
  <TerminalViewControls
    compact={false}
    showFit={false}
    showScrollToBottom
    onFit={noop}
    onScrollToBottom={noop}
  />,
);
assert(!fullTerminal.includes("terminal-fit"));
assert.match(fullTerminal, /bottom-2 right-2/);
assert(fullTerminal.includes("terminal-scroll-bottom"));

const embeddedTerminal = renderToStaticMarkup(
  <TerminalViewControls
    compact={false}
    showFit
    showScrollToBottom={false}
    onFit={noop}
    onScrollToBottom={noop}
  />,
);
assert(embeddedTerminal.includes("terminal-fit"));
assert(!embeddedTerminal.includes("terminal-scroll-bottom"));
