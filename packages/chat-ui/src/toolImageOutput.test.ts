import {
  isImageOnlyToolOutput,
  parseToolOutputImages,
  toolResultPreviewLine,
} from "./toolImageOutput";

const PNG_1X1_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

type TestFn = () => void;

const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

test("parseToolOutputImages accepts raw data URL", () => {
  const url = `data:image/png;base64,${PNG_1X1_BASE64}`;
  const parsed = parseToolOutputImages(url);
  assert(parsed.images.length === 1, "expected one image");
  assert(parsed.images[0].mimeType === "image/png", "expected png mime");
  assert(parsed.images[0].base64 === PNG_1X1_BASE64, "expected base64 payload");
  assert(parsed.remainder === null, "expected no remainder");
});

test("parseToolOutputImages accepts image_url JSON array", () => {
  const url = `data:image/png;base64,${PNG_1X1_BASE64}`;
  const payload = JSON.stringify([{ type: "image_url", image_url: { url } }]);
  const parsed = parseToolOutputImages(payload);
  assert(parsed.images.length === 1, "expected one image from json array");
  assert(parsed.images[0].dataUrl === url, "expected normalized data url");
  assert(parsed.remainder === null, "expected no remainder");
});

test("parseToolOutputImages keeps non-image text as remainder", () => {
  const parsed = parseToolOutputImages("Exit code: 0\nAll tests passed");
  assert(parsed.images.length === 0, "expected no images");
  assert(parsed.remainder === "Exit code: 0\nAll tests passed", "expected original text");
});

test("toolResultPreviewLine hides base64 tails", () => {
  const url = `data:image/png;base64,${PNG_1X1_BASE64}`;
  assert(toolResultPreviewLine(url) === "Image (PNG)", "image-only preview label");
  const json = JSON.stringify([{ type: "image_url", image_url: { url } }]);
  assert(toolResultPreviewLine(json) === "Image (PNG)", "json image preview label");
});

test("isImageOnlyToolOutput detects image-only payloads", () => {
  const url = `data:image/png;base64,${PNG_1X1_BASE64}`;
  assert(isImageOnlyToolOutput(url), "raw data url is image-only");
  assert(!isImageOnlyToolOutput("done"), "plain text is not image-only");
});

test("structured image results preserve every payload and surrounding text", () => {
  const url = `data:image/png;base64,${PNG_1X1_BASE64}`;
  for (const image of [
    { type: "input_image", image_url: url, detail: "original" },
    { type: "image_url", image_url: { url } },
    { type: "image", source: { type: "base64", media_type: "image/png", data: PNG_1X1_BASE64 } },
  ]) {
    const parsed = parseToolOutputImages(JSON.stringify([{ type: "text", text: "opaque-before" }, image, { type: "text", text: "opaque-after" }]));
    assert(parsed.images.length === 1, "one image survives structured parsing");
    assert(parsed.images[0].dataUrl === url, "all image bytes survive");
    assert(parsed.remainder === "opaque-before\nopaque-after", "text blocks survive alongside images");
    assert(!isImageOnlyToolOutput(JSON.stringify([{ type: "text", text: "opaque" }, image])), "mixed output retains its text identity");
  }
});

let failed = 0;
for (const [name, fn] of tests) {
  try {
    fn();
    console.log(`ok - ${name}`);
  } catch (error) {
    failed += 1;
    console.error(`FAIL - ${name}:`, error);
  }
}

if (failed > 0) {
  process.exit(1);
}

console.log(`ok - toolImageOutput (${tests.length} tests)`);
