import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ChatHighlightBudgetProvider, ChatHighlightRow } from "./ChatHighlightBudget";
import { CodeBlock } from "./CodeBlock";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLElement;
let root: Root;
const source = Array(20).fill("const answer = { value: 42, text: 'hello' };\n").join("");
const observations = new Set<FakeIntersection>();
class FakeIntersection {
  callback: IntersectionObserverCallback;
  constructor(callback: IntersectionObserverCallback) { this.callback = callback; observations.add(this); }
  observe() {}
  disconnect() { observations.delete(this); }
  deliver(visible: boolean) { this.callback([{ isIntersecting: visible } as IntersectionObserverEntry], this as unknown as IntersectionObserver); }
}

beforeEach(() => {
  vi.stubGlobal("IntersectionObserver", undefined);
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); vi.unstubAllGlobals(); });

describe("highlighting mounted code", () => {
  it("keeps actual Prism token spans within row and chat caps and releases allocations", async () => {
    const render = async (count: number) => act(async () => root.render(<ChatHighlightBudgetProvider>
      {Array.from({ length: count }, (_, row) => <ChatHighlightRow key={row} id={`r${row}`}>
        <section data-row={row}>{Array.from({ length: 12 }, (_, block) => <CodeBlock key={block} text={source} language="typescript" />)}</section>
      </ChatHighlightRow>)}
    </ChatHighlightBudgetProvider>));
    await render(4);
    const highlighted = () => host.querySelectorAll("span.token").length;
    expect(highlighted()).toBeGreaterThan(0);
    expect(highlighted()).toBeLessThanOrEqual(5000);
    for (const row of host.querySelectorAll("section")) expect(row.querySelectorAll("span.token").length).toBeLessThanOrEqual(2000);
    expect(Array.from(host.querySelectorAll("code")).some((block) => !block.querySelector("span.token") && block.textContent === source)).toBe(true);
    await render(1);
    expect(highlighted()).toBeGreaterThan(0);
    expect(highlighted()).toBeLessThanOrEqual(2000);
  });
  it("uses plain full source outside the viewport and highlights on visibility", async () => {
    vi.stubGlobal("IntersectionObserver", FakeIntersection);
    await act(async () => root.render(<ChatHighlightBudgetProvider><ChatHighlightRow id="row"><CodeBlock text={source} language="typescript" /></ChatHighlightRow></ChatHighlightBudgetProvider>));
    expect(host.querySelectorAll("span.token")).toHaveLength(0);
    expect(host.textContent).toContain(source);
    await act(async () => { for (const observer of observations) observer.deliver(true); });
    expect(host.querySelectorAll("span.token").length).toBeGreaterThan(0);
    await act(async () => { for (const observer of observations) observer.deliver(false); });
    expect(host.querySelectorAll("span.token")).toHaveLength(0);
    expect(host.textContent).toContain(source);
  });
  it("keeps large source plain, copies it intact, and offers explicit highlighting", async () => {
    const large = Array(220).fill("answer\n").join("");
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    await act(async () => root.render(<ChatHighlightBudgetProvider><ChatHighlightRow id="row"><CodeBlock text={large} language="typescript" /></ChatHighlightRow></ChatHighlightBudgetProvider>));
    expect(host.querySelectorAll("span.token")).toHaveLength(0);
    const copy = Array.from(host.querySelectorAll("button")).find((button) => button.textContent?.includes("Copy"))!;
    await act(async () => copy.click());
    expect(writeText).toHaveBeenCalledWith(large);
    const expand = host.querySelector<HTMLButtonElement>('[aria-label="Highlight code"]')!;
    expect(expand).toBeTruthy();
    await act(async () => expand.click());
    expect(host.textContent).toContain(large);
  });
});
