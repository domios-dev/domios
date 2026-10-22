import { lazy, memo, Suspense, type ComponentType } from "react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import { BrandState } from "../brand/BrandState";
import { brandStateModel } from "../brand/brandStateModel";

export function deferredComponent<Props extends object>(load: () => Promise<{ default: ComponentType<Props> }>) {
  const Component = lazy(load);
  // Panels stay mounted while hidden; an unchanged prop set must not re-render them with the app shell.
  return memo(function DeferredComponent(props: Props) {
    return <Suspense fallback={<DeferredLoader state="present" className="h-full w-full">
      <BrandState model={brandStateModel({ loading: true, label: "Loading" })} />
    </DeferredLoader>}>
      <Component {...props} />
    </Suspense>;
  });
}
