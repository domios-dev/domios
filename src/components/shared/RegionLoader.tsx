import type { HTMLAttributes } from "react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { BrandState } from "../brand/BrandState";
import { brandStateModel } from "../brand/brandStateModel";

/** The one loader for a region that is still loading: the brand mark with its dot waves and a styled status line. */
export function RegionLoader({ label, detail, className, ...attributes }: { label: string; detail?: string } & HTMLAttributes<HTMLDivElement>) {
  return <DeferredLoader state="present" className={`h-full w-full ${className ?? ""}`.trim()} {...attributes}>
    <BrandState model={brandStateModel({ loading: true, label, detail })} />
  </DeferredLoader>;
}
