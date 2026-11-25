// Run: npx tsx src/components/shared/Segmented.accessibility.test.tsx

import { act, useState } from "react";
import { Window } from "happy-dom";
import { createRoot } from "react-dom/client";
import { Segmented, type SegmentedOption } from "./Segmented";

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { value: true, configurable: true });

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

type Choice = "chat" | "terminal" | "blocked";

function ControlledSegmented({
  iconOnly,
  onChoice,
}: {
  iconOnly?: boolean;
  onChoice: (value: Choice) => void;
}) {
  const [value, setValue] = useState<Choice>("chat");
  const options: readonly SegmentedOption<Choice>[] = [
    { value: "chat", label: "Chat", icon: <svg aria-hidden="true" /> },
    { value: "terminal", label: "Terminal", icon: <svg aria-hidden="true" /> },
    { value: "blocked", label: "Unavailable", icon: <svg aria-hidden="true" />, disabled: true },
  ];
  return (
    <Segmented
      value={value}
      options={options}
      iconOnly={iconOnly}
      height={26}
      onChange={(next) => {
        onChoice(next);
        setValue(next);
      }}
      data-testid="tab-mode-switch"
      data-tab-id="tab-a"
      aria-label="Tab view"
    />
  );
}

async function main() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const choices: Choice[] = [];

  await act(async () => {
    root.render(<ControlledSegmented onChoice={(value) => choices.push(value)} />);
  });

  const switcher = host.querySelector('[data-testid="tab-mode-switch"]');
  assert(switcher?.getAttribute("role") === "tablist", "switch is a tablist");
  assert(switcher.getAttribute("aria-label") === "Tab view", "switch is named");
  assert(switcher.getAttribute("data-tab-id") === "tab-a", "switch exposes Tab scope");
  const chat = host.querySelector<HTMLButtonElement>('[role="tab"][data-value="chat"]');
  const terminal = host.querySelector<HTMLButtonElement>('[role="tab"][data-value="terminal"]');
  const unavailable = host.querySelector<HTMLButtonElement>('[role="tab"][data-value="blocked"]');
  assert(chat && terminal && unavailable, "all segments rendered");
  assert(chat.getAttribute("aria-selected") === "true" && chat.tabIndex === 0, "active view is exposed with roving tabindex");
  assert(terminal.getAttribute("aria-selected") === "false" && terminal.tabIndex === -1, "inactive view is exposed");
  assert(chat.tagName === "BUTTON" && terminal.tagName === "BUTTON", "segments are keyboard-operable buttons");
  assert(unavailable.disabled, "unavailable segment must be disabled");

  await act(async () => {
    terminal.click();
  });
  await act(async () => unavailable.click());
  assert(choices.length === 1 && choices[0] === "terminal", "a choice fires once and disabled segments never select");

  const thumbAfterClick = host.querySelector<HTMLElement>(".ct-liquid-segmented__thumb");
  assert(thumbAfterClick, "thumb renders after click");
  assert(thumbAfterClick.classList.contains("ct-liquid-segmented__thumb--travel"), "click triggers liquid travel animation");
  assert(thumbAfterClick.style.getPropertyValue("--sx-from") === "0%", "travel starts from previous index");
  assert(thumbAfterClick.style.getPropertyValue("--sx-to") === "100%", "travel ends at target index");
  await act(async () => {
    thumbAfterClick.dispatchEvent(new window.Event("animationend", { bubbles: true }) as unknown as Event);
  });
  assert(
    !host.querySelector(".ct-liquid-segmented__thumb")?.classList.contains("ct-liquid-segmented__thumb--travel"),
    "travel class clears on animationend",
  );

  const key = (code: string) => new window.KeyboardEvent("keydown", { code, bubbles: true, cancelable: true });
  await act(async () => {
    terminal.focus();
    terminal.dispatchEvent(key("ArrowRight") as unknown as Event);
  });
  assert(chat.getAttribute("aria-selected") === "true" && document.activeElement === chat, "ArrowRight skips disabled and wraps to Chat");
  const thumbAfterArrow = host.querySelector<HTMLElement>(".ct-liquid-segmented__thumb");
  assert(thumbAfterArrow, "thumb renders after arrow navigation");
  assert(thumbAfterArrow.classList.contains("ct-liquid-segmented__thumb--travel"), "arrow navigation triggers liquid travel animation");
  assert(thumbAfterArrow.style.getPropertyValue("--sx-from") === "100%", "reverse travel starts from terminal");
  assert(thumbAfterArrow.style.getPropertyValue("--sx-to") === "0%", "reverse travel ends at chat");

  await act(async () => {
    chat.dispatchEvent(key("End") as unknown as Event);
  });
  assert(terminal.getAttribute("aria-selected") === "true" && document.activeElement === terminal, "End lands on last enabled segment");
  await act(async () => {
    terminal.dispatchEvent(key("Home") as unknown as Event);
  });
  assert(chat.getAttribute("aria-selected") === "true" && document.activeElement === chat, "Home lands on first enabled segment");

  await act(async () => {
    root.render(<ControlledSegmented iconOnly onChoice={(value) => choices.push(value)} />);
  });
  const iconSwitch = host.querySelector<HTMLElement>('[data-testid="tab-mode-switch"]');
  assert(iconSwitch, "iconOnly switch rendered");
  assert(iconSwitch.classList.contains("ct-liquid-segmented--icon-only"), "iconOnly adds icon-only modifier class");
  const iconChat = iconSwitch.querySelector<HTMLButtonElement>('[role="tab"][data-value="chat"]');
  const iconTerminal = iconSwitch.querySelector<HTMLButtonElement>('[role="tab"][data-value="terminal"]');
  assert(iconChat && iconTerminal, "iconOnly renders buttons");
  assert(iconChat.textContent === "" && iconTerminal.textContent === "", "iconOnly hides visible label text");
  assert(iconChat.getAttribute("aria-label") === "Chat" && iconTerminal.getAttribute("aria-label") === "Terminal", "iconOnly promotes labels to accessible names");
  assert(iconChat.title === "Chat" && iconTerminal.title === "Terminal", "iconOnly preserves tooltips");

  await act(async () => root.unmount());
  console.log("ok - liquid segmented accessibility and motion contract");
}

void main();
