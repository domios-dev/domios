import assert from "node:assert/strict";
import { formatModelBadge, modelBadgeWidthForProvider, MODEL_BADGE_MAX_CHARS } from "./formatModelBadge";

const providerWidth = modelBadgeWidthForProvider("opencode");
const cappedWidth = modelBadgeWidthForProvider("a-very-long-provider-label");
const badge = formatModelBadge("ACME/THIS-MODEL-FAMILY-NAME-IS-TOO-LONG-120B", providerWidth);

assert.equal(formatModelBadge("GOOGLE/GEMMA-4-26B-A4B-QAT"), "GEMMA-4-26B");
assert.ok(providerWidth >= "opencode".length && providerWidth <= MODEL_BADGE_MAX_CHARS);
assert.equal(cappedWidth, MODEL_BADGE_MAX_CHARS);
assert.ok(badge.length <= providerWidth);
