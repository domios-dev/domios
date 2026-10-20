import { afterEach, expect, it, vi } from "vitest";
import { DEFAULT_WINDOWS_BUILD_NUMBER, knownWindowsBuildNumber, readWindowsBuildNumber, resetWindowsBuildNumberForTests } from "./windowsBuildNumber";

afterEach(resetWindowsBuildNumberForTests);

it("asks the host once and serves later terminal mounts from the first answer", async () => {
  const read = vi.fn(async () => 22631);
  expect(knownWindowsBuildNumber()).toBe(DEFAULT_WINDOWS_BUILD_NUMBER);
  const [first, second] = await Promise.all([readWindowsBuildNumber(read), readWindowsBuildNumber(read)]);
  expect(await readWindowsBuildNumber(read)).toBe(22631);
  expect([first, second]).toEqual([22631, 22631]);
  expect(knownWindowsBuildNumber()).toBe(22631);
  expect(read).toHaveBeenCalledTimes(1);
});

it("keeps the default when the host cannot report its build", async () => {
  const read = vi.fn(async () => { throw new Error("unavailable"); });
  expect(await readWindowsBuildNumber(read)).toBe(DEFAULT_WINDOWS_BUILD_NUMBER);
  expect(await readWindowsBuildNumber(read)).toBe(DEFAULT_WINDOWS_BUILD_NUMBER);
  expect(read).toHaveBeenCalledTimes(1);
});
