import {
  quotePathForPaste,
  looksLikeAbsolutePath,
  fileUriToPath,
  extractFilePathsFromClipboard,
} from "./clipboardPaste";

let passed = 0;
let failed = 0;

function assert(label: string, actual: string, expected: string) {
  if (actual === expected) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected "${expected}", got "${actual}"`);
    failed++;
  }
}

function assertEq<T>(label: string, actual: T, expected: T) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a === e) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${e}, got ${a}`);
    failed++;
  }
}

function assertBool(label: string, actual: boolean, expected: boolean) {
  if (actual === expected) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}: expected ${expected}, got ${actual}`);
    failed++;
  }
}

function fakeClipboard(types: Record<string, string>): DataTransfer {
  return {
    getData(type: string): string {
      return types[type] ?? "";
    },
  } as unknown as DataTransfer;
}

console.log("quotePathForPaste:");

assert("plain ascii unchanged",
  quotePathForPaste("/tmp/foo.png"),
  "/tmp/foo.png");

assert("space → single-quote wrap",
  quotePathForPaste("/Users/me/Hello World.png"),
  "'/Users/me/Hello World.png'");

assert("cyrillic + space → single-quote wrap (utf-8 preserved)",
  quotePathForPaste("/Users/rollacode/Desktop/Снимок экрана 2026-05-05 в 10.49.45.png"),
  "'/Users/rollacode/Desktop/Снимок экрана 2026-05-05 в 10.49.45.png'");

assert("windows backslash without space → unchanged (cmd.exe compatibility)",
  quotePathForPaste("C:\\Users\\me\\file.png"),
  "C:\\Users\\me\\file.png");

assert("windows backslash + space → single-quote wrap",
  quotePathForPaste("C:\\Users\\me\\file with space.png"),
  "'C:\\Users\\me\\file with space.png'");

assert("windows path with $ → quoted",
  quotePathForPaste("C:\\$Recycle.Bin\\file.png"),
  "'C:\\$Recycle.Bin\\file.png'");

assert("internal single quote → double-quote wrap with escapes",
  quotePathForPaste("/tmp/it's broken.png"),
  '"/tmp/it\'s broken.png"');

assert("dollar sign in path → quoted",
  quotePathForPaste("/tmp/$foo.png"),
  "'/tmp/$foo.png'");

assert("ampersand → quoted",
  quotePathForPaste("/tmp/a&b.png"),
  "'/tmp/a&b.png'");

assert("parens → quoted",
  quotePathForPaste("/tmp/v(1).png"),
  "'/tmp/v(1).png'");

assert("hash → quoted",
  quotePathForPaste("/tmp/#draft.png"),
  "'/tmp/#draft.png'");

assert("tab unchanged when only safe chars",
  quotePathForPaste("/tmp/safe-name_123.png"),
  "/tmp/safe-name_123.png");

assert("dot leading still safe",
  quotePathForPaste("/tmp/.hidden.png"),
  "/tmp/.hidden.png");

console.log("\nlooksLikeAbsolutePath:");

assertBool("unix absolute path", looksLikeAbsolutePath("/Users/me/foo.png"), true);
assertBool("windows backslash drive", looksLikeAbsolutePath("C:\\Users\\me\\foo.png"), true);
assertBool("windows forward-slash drive", looksLikeAbsolutePath("C:/Users/me/foo.png"), true);
assertBool("home alias", looksLikeAbsolutePath("~/foo.png"), true);
assertBool("leading whitespace tolerated", looksLikeAbsolutePath("  /tmp/foo  "), true);
assertBool("relative path rejected", looksLikeAbsolutePath("relative/path.png"), false);
assertBool("empty string rejected", looksLikeAbsolutePath(""), false);
assertBool("https URL rejected", looksLikeAbsolutePath("https://example.com/x.png"), false);
assertBool("drive letter alone rejected", looksLikeAbsolutePath("C:foo"), false);
assertBool("path with embedded newline rejected", looksLikeAbsolutePath("/tmp/a\n/tmp/b"), false);
assertBool("very long string rejected", looksLikeAbsolutePath("/" + "a".repeat(1001)), false);
assertBool("just /", looksLikeAbsolutePath("/"), true);
assertBool("just C:\\", looksLikeAbsolutePath("C:\\"), true);

console.log("\nfileUriToPath:");

assertEq("non-file uri rejected", fileUriToPath("https://x/y"), null);
assertEq("plain path without scheme rejected", fileUriToPath("/Users/me/foo.png"), null);
assertEq("macos file uri", fileUriToPath("file:///Users/me/photo.png"), "/Users/me/photo.png");
assertEq("windows file uri strips leading slash", fileUriToPath("file:///C:/Users/me/foo.png"), "C:/Users/me/foo.png");
assertEq("windows file uri with backslash drive", fileUriToPath("file:///C:\\Users\\me\\foo.png"), "C:\\Users\\me\\foo.png");
assertEq("percent-encoded space", fileUriToPath("file:///Users/me/my%20photo.png"), "/Users/me/my photo.png");
assertEq("percent-encoded cyrillic", fileUriToPath("file:///Users/me/%D0%A1%D0%BD%D0%B8%D0%BC%D0%BE%D0%BA.png"), "/Users/me/Снимок.png");
assertEq("malformed percent encoding falls back to raw", fileUriToPath("file:///Users/me/bad%ZZ.png"), "/Users/me/bad%ZZ.png");
assertEq("empty file:// returns empty", fileUriToPath("file://"), "");

console.log("\nextractFilePathsFromClipboard:");

assertEq("empty clipboard", extractFilePathsFromClipboard(fakeClipboard({})), []);
assertEq("macOS Finder single file via uri-list",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/uri-list": "file:///Users/me/photo.png",
  })),
  ["/Users/me/photo.png"]);

assertEq("macOS Finder multi-file CRLF",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/uri-list": "file:///Users/me/a.png\r\nfile:///Users/me/b.png\r\n",
  })),
  ["/Users/me/a.png", "/Users/me/b.png"]);

assertEq("RFC 2483 comments are skipped",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/uri-list": "# this is a comment\nfile:///Users/me/photo.png\n# trailing\n",
  })),
  ["/Users/me/photo.png"]);

assertEq("Windows Explorer single file (file:///C:/...)",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/uri-list": "file:///C:/Users/Andrey/Pictures/screenshot.png",
  })),
  ["C:/Users/Andrey/Pictures/screenshot.png"]);

assertEq("plain text path falls back when uri-list empty",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/plain": "/Users/me/photo.png",
  })),
  ["/Users/me/photo.png"]);

assertEq("plain text URL is NOT a path",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/plain": "https://example.com/photo.png",
  })),
  []);

assertEq("uri-list takes priority over text/plain",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/uri-list": "file:///Users/me/from-uri.png",
    "text/plain": "/Users/me/from-text.png",
  })),
  ["/Users/me/from-uri.png"]);

assertEq("dedup identical URIs",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/uri-list": "file:///Users/me/x.png\nfile:///Users/me/x.png\n",
  })),
  ["/Users/me/x.png"]);

assertEq("uri-list with cyrillic filename",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/uri-list": "file:///Users/me/%D0%A1%D0%BD%D0%B8%D0%BC%D0%BE%D0%BA%20%D1%8D%D0%BA%D1%80%D0%B0%D0%BD%D0%B0.png",
  })),
  ["/Users/me/Снимок экрана.png"]);

assertEq("plain text with newline is NOT a path attachment",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/plain": "/Users/me/photo.png\nhello",
  })),
  []);

assertEq("trims surrounding whitespace from plain text path",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/plain": "  /Users/me/photo.png  ",
  })),
  ["/Users/me/photo.png"]);

assertEq("Windows backslash plain text path",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/plain": "C:\\Users\\Andrey\\photo.png",
  })),
  ["C:\\Users\\Andrey\\photo.png"]);

assertEq("uri-list mixed with non-file URI keeps only file URIs",
  extractFilePathsFromClipboard(fakeClipboard({
    "text/uri-list": "https://example.com/x.png\nfile:///Users/me/photo.png\n",
  })),
  ["/Users/me/photo.png"]);

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
