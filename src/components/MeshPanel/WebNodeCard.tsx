import type { ComponentProps } from "react";
import { WebAccessPanel } from "./WebAccessPanel";

type Props = Omit<ComponentProps<typeof WebAccessPanel>, "embedded" | "onClose"> & { onDetails?: () => void };

export function WebNodeCard({ onDetails, ...props }: Props) {
  return <>
    <WebAccessPanel {...props} embedded onClose={() => {}} />
    {onDetails && <button type="button" onClick={onDetails} className="mt-2 w-full rounded border border-[var(--ct-border)] bg-[var(--ct-bg)] p-2 text-sm">Web details</button>}
  </>;
}
