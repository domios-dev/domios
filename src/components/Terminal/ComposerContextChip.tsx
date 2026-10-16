import type { ComponentPropsWithRef } from "react";

export function ComposerContextChip({ kind, open, children, ...props }: ComponentPropsWithRef<"button"> & {
  kind: string;
  open: boolean;
}) {
  return <button type="button" className="ct-ctx-kind" data-kind={kind} data-open={open || undefined}
    onMouseDown={(event) => event.preventDefault()} {...props}>{children}</button>;
}
