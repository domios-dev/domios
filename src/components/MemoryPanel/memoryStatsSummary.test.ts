import assert from "node:assert/strict";
import { memoryStatsSummary } from "./memoryStatsSummary";

assert.equal(memoryStatsSummary(1, 1, 1), "1 memory · 1 connection · 1 indexed");
assert.equal(memoryStatsSummary(0, 0, 0), "0 memories · 0 connections · 0 indexed");
assert.equal(memoryStatsSummary(1181, 12430, 4425), "1,181 memories · 12,430 connections · 4,425 indexed");
assert.equal(memoryStatsSummary(1, 2, 0), "1 memory · 2 connections · 0 indexed");
assert.equal(memoryStatsSummary(2, 1, 0), "2 memories · 1 connection · 0 indexed");

console.log("✓ Memory summary formats singular, plural, zero and grouped counts");
