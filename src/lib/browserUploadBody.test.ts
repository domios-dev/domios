import assert from "node:assert/strict";
import test from "node:test";
import { browserUploadBody } from "./browserUploadBody";

test("images and voice bytes use one browser multipart field with filename and original MIME", async () => {
  for (const [type, name, payload] of [["image/png", "phone photo.png", "pixels"], ["audio/webm", "voice-note.webm", "voice-bytes"]]) {
    const body = await browserUploadBody(new Blob([payload], { type }), name);
    const field = body.get("file") as File;
    assert.equal(field.name, name); assert.equal(field.type, type); assert.equal(await field.text(), payload);
    const request = new Request("https://example.invalid/upload", { method: "POST", body });
    assert.match(request.headers.get("content-type")!, /^multipart\/form-data; boundary=/);
    const roundTrip = (await request.formData()).get("file") as File;
    assert.equal(await roundTrip.text(), payload);
  }
});

test("a truncated browser blob is rejected before a malformed request can be sent", async () => {
  const file = new Blob(["phone"]);
  file.arrayBuffer = async () => new ArrayBuffer(0);
  await assert.rejects(browserUploadBody(file, "phone.png"), TypeError);
});
