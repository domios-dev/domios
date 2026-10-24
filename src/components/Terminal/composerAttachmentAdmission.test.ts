import { describe, expect, it } from "vitest";
import { formatAttachmentMarker } from "@codeterm/chat-core";
import { composerAttachmentError } from "./composerAttachmentAdmission";
import { capsuleToken } from "./composerCapsules";

describe("composer attachment capabilities", () => {
  const paths = { filePaths: true, imagePaths: true, inlineText: true };
  const inline = { filePaths: false, imagePaths: false, inlineText: true };
  it.each(["/opaque/file.txt", "/opaque/picture.png"])("admits readable paths: %s", path => {
    expect(composerAttachmentError(paths, "inspect", [path], [], {})).toBeNull();
    expect(composerAttachmentError(inline, "inspect", [path], [], {})).toContain("cannot read");
    expect(composerAttachmentError(undefined, "inspect", [path], [], {})).toContain("unavailable");
  });
  it("treats file and image support independently", () => {
    expect(composerAttachmentError({ ...paths, imagePaths: false }, "", ["a.png"], [], {})).toContain("image");
    expect(composerAttachmentError({ ...paths, filePaths: false }, "", ["a.png"], [], {})).toBeNull();
    expect(composerAttachmentError({ ...paths, filePaths: false }, "", ["a.txt"], [], {})).toContain("file");
  });
  it("checks active and uploading capsules, portable markers and reply files", () => {
    const registry = { note: { label: "note", path: "", kind: "text" as const, uploadId: 3 } };
    expect(composerAttachmentError(inline, capsuleToken("note"), [], [], registry)).toContain("Paste text directly");
    expect(composerAttachmentError(inline, "plain", [], [], registry)).toBeNull();
    expect(composerAttachmentError(inline, formatAttachmentMarker("a.png"), [], [], {})).toContain("image");
    expect(composerAttachmentError(inline, "plain", [], [], {}, { label: "quote", excerpt: "opaque", ref: "m", path: "a.txt" })).toContain("file");
  });
  it("admits inline text and transcribed recordings without readable paths", () => {
    expect(composerAttachmentError(inline, "opaque\n".repeat(500), ["a.webm"], ["a.webm"], {})).toBeNull();
    expect(composerAttachmentError(inline, "opaque", ["a.webm"], [], {})).toContain("file");
    expect(composerAttachmentError(undefined, "plain", [], [], {})).toBeNull();
  });
});
