import { lazy, Suspense, type ComponentType } from "react";
import { DeferredLoader } from "./DeferredLoader";
import { BrandState } from "../brand/BrandState";
import { brandStateModel } from "../brand/brandStateModel";

export function deferredComponent<Props extends object>(load: () => Promise<{ default: ComponentType<Props> }>) {
  const Component = lazy(load);
  return function DeferredComponent(props: Props) {
    return <Suspense fallback={<DeferredLoader state="present" className="h-full w-full">
      <BrandState model={brandStateModel({ loading: true, label: "Loading" })} />
    </DeferredLoader>}>
      <Component {...props} />
    </Suspense>;
  };
}
