import type { HTMLAttributes } from "react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { BusyState } from "./BusyState";

/** The one loader for a region that is still loading: the shared activity dots with a quiet caption, centred in the
 *  region and shown only if loading outlasts the deferral, so a fast load never flashes it. The app's boot and restore
 *  screens are the only places that show the brand instead. */
export function RegionLoader({ label, className, ...attributes }: { label: string } & HTMLAttributes<HTMLDivElement>) {
  return <DeferredLoader state="present" className={`flex h-full w-full items-center justify-center ${className ?? ""}`.trim()} {...attributes}>
    <BusyState inline label={label} phase="work" />
  </DeferredLoader>;
}
