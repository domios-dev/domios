import { lazy, Suspense, type ComponentType } from "react";
import { DeferredLoader } from "./DeferredLoader";
import { BusyState } from "./BusyState";

export function deferredComponent<Props extends object>(load: () => Promise<{ default: ComponentType<Props> }>) {
  const Component = lazy(load);
  return function DeferredComponent(props: Props) {
    return <Suspense fallback={<DeferredLoader state="present"><BusyState label="Loading view" /></DeferredLoader>}>
      <Component {...props} />
    </Suspense>;
  };
}
