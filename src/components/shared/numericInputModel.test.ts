// Run: npx tsx src/components/shared/numericInputModel.test.ts

import { describe, expect, finish, it } from "../../test/standaloneTest";
import {
  formatNumericInput,
  lastCommittedNumeric,
  parseNumericInput,
  reconcileExternalNumericValue,
  stepNumericValue,
  type PendingNumericEmission,
} from "./numericInputModel";

describe("numericInputModel", () => {
  it("formats finite numbers in locale-independent dot-decimal form", () => {
    expect(formatNumericInput(0.15)).toBe("0.15");
    expect(formatNumericInput(1800)).toBe("1800");
    expect(formatNumericInput(0)).toBe("0");
    expect(formatNumericInput(-0)).toBe("0");
    expect(formatNumericInput(null)).toBe("");
    expect(formatNumericInput(undefined)).toBe("");
    expect(formatNumericInput(Number.NaN)).toBe("");
    expect(formatNumericInput(Number.POSITIVE_INFINITY)).toBe("");
  });

  it("parses plain dot-decimal inputs and preserves round trips within bounds", () => {
    expect(parseNumericInput("0.15", { min: 0.05, max: 0.5 })).toBe(0.15);
    expect(parseNumericInput(".15", { min: 0.05, max: 0.5 })).toBe(0.15);
    expect(parseNumericInput(" 0.05 ", { min: 0.05, max: 0.5 })).toBe(0.05);
    expect(parseNumericInput("0.5", { min: 0.05, max: 0.5 })).toBe(0.5);
    expect(parseNumericInput("0.150", { min: 0.05, max: 0.5 })).toBe(0.15);
    expect(parseNumericInput("1800", { integer: true, min: 10, max: 3600 })).toBe(1800);
  });

  it("rejects locale commas, incomplete drafts, scientific notation, and out-of-range values", () => {
    expect(parseNumericInput("0,15", { min: 0.05, max: 0.5 })).toBeNull();
    expect(parseNumericInput("1,800", { integer: true })).toBeNull();
    expect(parseNumericInput("0.", { min: 0, max: 1 })).toBeNull();
    expect(parseNumericInput("", { min: 0, max: 1 })).toBeNull();
    expect(parseNumericInput("   ", { min: 0, max: 1 })).toBeNull();
    expect(parseNumericInput("1e-1", { min: 0, max: 1 })).toBeNull();
    expect(parseNumericInput("0.04", { min: 0.05, max: 0.5 })).toBeNull();
    expect(parseNumericInput("0.51", { min: 0.05, max: 0.5 })).toBeNull();
    expect(parseNumericInput("10.5", { integer: true, min: 1, max: 100 })).toBeNull();
  });

  it("steps decimals without binary float drift and clamps to bounds", () => {
    expect(stepNumericValue(0.15, 1, { step: 0.01, min: 0.05, max: 0.5 })).toBe(0.16);
    expect(stepNumericValue(0.15, -1, { step: 0.01, min: 0.05, max: 0.5 })).toBe(0.14);
    expect(stepNumericValue(0.05, -1, { step: 0.01, min: 0.05, max: 0.5 })).toBe(0.05);
    expect(stepNumericValue(0.5, 1, { step: 0.01, min: 0.05, max: 0.5 })).toBe(0.5);
    expect(stepNumericValue(60, 1, { integer: true, min: 5, max: 1440 })).toBe(61);
  });

  it("reconciles delayed echoes without overwriting focused or pending local drafts", () => {
    const queue: PendingNumericEmission[] = [
      { kind: "value", value: 0.19 },
      { kind: "value", value: 0.18 },
      { kind: "value", value: 0.15 },
    ];
    expect(lastCommittedNumeric(queue, 0.2)).toEqual({ kind: "value", value: 0.15 });

    const focusedEarlyEcho = reconcileExternalNumericValue(queue, 0.19, true);
    expect(focusedEarlyEcho.nextDraft).toBeNull();
    expect(focusedEarlyEcho.pending).toEqual([
      { kind: "value", value: 0.18 },
      { kind: "value", value: 0.15 },
    ]);

    const blurredEarlyEcho = reconcileExternalNumericValue(focusedEarlyEcho.pending, 0.18, false);
    expect(blurredEarlyEcho.nextDraft).toBeNull();
    expect(blurredEarlyEcho.pending).toEqual([{ kind: "value", value: 0.15 }]);

    const blurredFinalEcho = reconcileExternalNumericValue(blurredEarlyEcho.pending, 0.15, false);
    expect(blurredFinalEcho.nextDraft).toBe("0.15");
    expect(blurredFinalEcho.pending).toEqual([]);

    const blurredExternalChange = reconcileExternalNumericValue([], 0.35, false);
    expect(blurredExternalChange.nextDraft).toBe("0.35");

    const focusedExternalChange = reconcileExternalNumericValue([], 0.35, true);
    expect(focusedExternalChange.nextDraft).toBeNull();
  });
});

finish();
