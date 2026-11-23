import { strict as assert } from "node:assert";
import { test } from "node:test";
import { PCM_RESAMPLER_SOURCE, PCM_SAMPLE_RATE } from "./pcmCapture";

interface Resampler { push(input: Float32Array, emit: (sample: number) => void): void }
const Resampler = new Function(`${PCM_RESAMPLER_SOURCE}; return PcmResampler;`)() as
  new (inputRate: number, outputRate: number) => Resampler;

function resample(rate: number, input: Float32Array, quantum = 128) {
  const resampler = new Resampler(rate, PCM_SAMPLE_RATE);
  const output: number[] = [];
  for (let offset = 0; offset < input.length; offset += quantum) {
    resampler.push(input.subarray(offset, offset + quantum), sample => output.push(sample));
  }
  return output;
}

const ramp = (length: number) => Float32Array.from({ length }, (_, index) => index / length);
const close = (actual: number, expected: number, epsilon: number, label: string) =>
  assert.ok(Math.abs(actual - expected) <= epsilon, `${label}: ${actual} != ${expected}`);

test("worklet resampler passes 16 kHz audio through unchanged", () => {
  const input = ramp(1000);
  assert.deepEqual(resample(PCM_SAMPLE_RATE, input), Array.from(input));
});

for (const rate of [48_000, 44_100, 24_000]) {
  test(`worklet resampler converts ${rate} Hz render quanta into continuous 16 kHz samples`, () => {
    const seconds = 2;
    const input = ramp(rate * seconds);
    const output = resample(rate, input);
    assert.ok(Math.abs(output.length - PCM_SAMPLE_RATE * seconds) <= 1, `${output.length} samples`);
    const step = rate / PCM_SAMPLE_RATE;
    output.forEach((sample, index) => close(sample, (index * step) / input.length, 1e-6, `sample ${index}`));
  });
}

test("worklet resampler output is independent of how the hardware splits render quanta", () => {
  const input = Float32Array.from({ length: 44_100 }, (_, index) => Math.sin(index / 7));
  const whole = resample(44_100, input, input.length);
  const split = resample(44_100, input, 97);
  assert.equal(split.length, whole.length);
  split.forEach((sample, index) => close(sample, whole[index], 1e-6, `sample ${index}`));
});
