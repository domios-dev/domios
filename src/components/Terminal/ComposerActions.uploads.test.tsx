import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { ComposerActions } from "./ComposerActions";
const upload = vi.hoisted(() => vi.fn());
const voiceConfig = vi.hoisted(() => ({ persistBlob: null as null | ((blob: Blob, name: string) => Promise<string>) }));
vi.mock("./voice/audio", async (original) => ({ ...(await original<typeof import("./voice/audio")>()), storeBlob: upload }));
vi.mock("./voice/useComposerVoice", () => ({ useComposerVoice: (config: typeof voiceConfig) => { voiceConfig.persistBlob = config.persistBlob; return { isPreview: false, mainIsSend: true }; } }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("keeps a failed picker attachment local and retries it into the attachment sink", async () => {
  upload.mockReset();
  upload.mockRejectedValueOnce(new TypeError("interrupted")).mockResolvedValueOnce("/uploads/photo.png");
  const attach = vi.fn(); const error = vi.fn();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ComposerActions onAttach={attach} onError={error} onSend={() => {}} onSendWithAttachments={() => {}} sendDisabled={false} hasText><textarea /></ComposerActions>));
    const input = host.querySelector<HTMLInputElement>('input[type="file"]')!;
    Object.defineProperty(input, "files", { value: [new File(["photo"], "photo.png", { type: "image/png" })] });
    await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
    expect(error).not.toHaveBeenCalled(); expect(attach).not.toHaveBeenCalled();
    const retry = host.querySelector<HTMLButtonElement>('[aria-label="Retry attachment photo.png"]')!;
    expect(retry).toBeTruthy();
    await act(async () => retry.click());
    expect(attach).toHaveBeenCalledExactlyOnceWith(["/uploads/photo.png"]);
    expect(host.querySelector('[aria-label="Retry attachment photo.png"]')).toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); }
});

it("a failed voice upload retries through the voice send handler once", async () => {
  upload.mockReset();
  upload.mockRejectedValueOnce(new TypeError("interrupted")).mockResolvedValueOnce("/uploads/voice.webm");
  const send = vi.fn(); const error = vi.fn();
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ComposerActions onAttach={() => {}} onError={error} onSend={() => {}} onSendWithAttachments={send} sendDisabled={false} hasText={false}><textarea /></ComposerActions>));
    await act(async () => { expect(await voiceConfig.persistBlob!(new Blob(["voice"], { type: "audio/webm" }), "voice.webm")).toBe(""); });
    expect(error).not.toHaveBeenCalled(); expect(send).not.toHaveBeenCalled();
    const retry = host.querySelector<HTMLButtonElement>('[aria-label="Retry attachment voice.webm"]')!;
    await act(async () => retry.click());
    expect(send).toHaveBeenCalledExactlyOnceWith(["/uploads/voice.webm"]);
    expect(host.querySelector('[aria-label="Retry attachment voice.webm"]')).toBeNull();
  } finally { await act(async () => root.unmount()); host.remove(); }
});
