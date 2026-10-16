import assert from "node:assert/strict";
import { utf8Bytes, sanitizeHistory } from "../src/runtimeHistoryBounds";

for (const value of ["", "ascii\0\n", "éЖ漢字", "🧪🌍", "\ud800", "\udc00", "\ud800x\udc00", "\ud800\ud800\udc00\udc00"]) {
  assert.equal(utf8Bytes(value), Buffer.byteLength(value), JSON.stringify(value));
}
const bmp = Array.from({ length: 65536 }, (_, point) => String.fromCharCode(point)).join("");
assert.equal(utf8Bytes(bmp), Buffer.byteLength(bmp));
const astral = Array.from({ length: 4096 }, (_, point) => String.fromCodePoint(0x10000 + point * 251)).join("");
assert.equal(utf8Bytes(astral), Buffer.byteLength(astral));
for (const character of ["x", "é", "漢", "🧪"]) {
  const limit = 16384 / Buffer.byteLength(character);
  const within = character.repeat(limit);
  assert.equal(sanitizeHistory(within), within);
  assert.notEqual(sanitizeHistory(within + character), within + character);
}
console.log("PASS history UTF-8 budgets: BMP, astral, lone surrogates, exact truncation boundary");
