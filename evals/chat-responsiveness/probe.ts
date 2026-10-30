import { Window } from "happy-dom";
import os from "node:os";
import { execFileSync } from "node:child_process";

const window = new Window({ url: "http://localhost/isolated-responsiveness" });
for (const name of ["window", "document", "navigator", "HTMLElement", "HTMLButtonElement", "HTMLTextAreaElement", "Node", "MouseEvent", "Event", "MutationObserver", "getComputedStyle", "localStorage"] as const) {
  Object.defineProperty(globalThis, name, { configurable: true, value: name === "window" ? window : window[name] });
}
Object.defineProperty(globalThis, "BroadcastChannel", { configurable: true, value: undefined });
const frames = new Map<number, FrameRequestCallback>();
let nextFrame = 0;
Object.assign(globalThis, {
  IS_REACT_ACT_ENVIRONMENT: true,
  requestAnimationFrame: (callback: FrameRequestCallback) => { frames.set(++nextFrame, callback); return nextFrame; },
  cancelAnimationFrame: (id: number) => frames.delete(id),
});
const flush = () => { const callbacks = [...frames.values()]; frames.clear(); for (const callback of callbacks) callback(0); };
async function main() {
const { runProbe } = await import("./probeFixture");
console.log(JSON.stringify({ source: execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim(), hardware: { cpu: os.cpus()[0]?.model, logicalCpus: os.cpus().length, memoryBytes: os.totalmem() }, timing: "CPU elapsed and mocked DOM commit; no browser paint or native runtime proof" }));
await runProbe(flush);
await window.happyDOM.close();

}
void main().catch(error => { console.error(error); process.exitCode = 1; });
