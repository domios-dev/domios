import assert from "node:assert/strict";
import { attachmentFileName, inlineAttachmentSrc, userVisibleText } from "./userAttachments";

const PATH = String.raw`C:\Users\User\AppData\Local\Temp\codeterm-chat\e8a97274-pasted-image-1791277476989.png`;
const pathImage = { id: "a", mimeType: "image/png", path: PATH };

assert.equal(userVisibleText("[1 image]", [pathImage]), "");
assert.equal(userVisibleText("[2 images]", [pathImage, pathImage]), "");
assert.equal(userVisibleText("[1 image]", []), "[1 image]");
assert.equal(userVisibleText("look at [1 image] here", [pathImage]), "look at [1 image] here");

assert.equal(attachmentFileName(pathImage), "e8a97274-pasted-image-1791277476989.png");
assert.equal(attachmentFileName({ id: "b", mimeType: "image/jpeg", path: "/tmp/shot.jpg" }), "shot.jpg");
assert.equal(attachmentFileName({ id: "c", mimeType: "image/webp", data: "AAAA" }), "image.webp");
assert.equal(attachmentFileName({ id: "d", mimeType: "image/png", url: "data:image/png;base64,AAAA" }), "image.png");

assert.equal(inlineAttachmentSrc({ id: "e", mimeType: "image/png", data: "AAAA" }), "data:image/png;base64,AAAA");
assert.equal(inlineAttachmentSrc({ id: "f", mimeType: "image/png", url: "data:image/png;base64,BBBB" }), "data:image/png;base64,BBBB");
assert.equal(inlineAttachmentSrc({ id: "g", mimeType: "image/png", url: "https://example.test/x.png" }), null);
assert.equal(inlineAttachmentSrc(pathImage), null);

console.log("userAttachments tests passed");
