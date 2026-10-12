import { expect, it, vi } from "vitest";
import { createRuntimeBinding } from "./runtimeBindingModel";

it("binds stable clients once and invalidates only on a real replacement", () => {
  const bind = vi.fn(), invalidate = vi.fn();
  const accept = createRuntimeBinding(bind, invalidate);
  const first = {}, next = {};
  for (let access = 0; access < 102; access++) accept(first);
  expect(bind).toHaveBeenCalledExactlyOnceWith(first);
  expect(invalidate).not.toHaveBeenCalled();
  accept(next);
  accept(next);
  expect(bind).toHaveBeenCalledTimes(2);
  expect(bind).toHaveBeenLastCalledWith(next);
  expect(invalidate).toHaveBeenCalledTimes(1);
});
