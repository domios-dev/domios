import { forwardRef, type HTMLAttributes } from "react";

export type DeferredLoaderState = "present" | "exiting";

interface DeferredLoaderProps extends HTMLAttributes<HTMLDivElement> {
  state: DeferredLoaderState;
}

export const DeferredLoader = forwardRef<HTMLDivElement, DeferredLoaderProps>(function DeferredLoader(
  { state, className, ...props },
  ref,
) {
  return (
    <div
      ref={ref}
      className={["ct-deferred-loader", className].filter(Boolean).join(" ")}
      data-transition-state={state}
      {...props}
    />
  );
});
