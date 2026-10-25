import { lazy, memo, Suspense, type ComponentType } from "react";
import { RegionLoader } from "./RegionLoader";

export function deferredComponent<Props extends object>(load: () => Promise<{ default: ComponentType<Props> }>) {
  const Component = lazy(load);
  // Panels stay mounted while hidden; an unchanged prop set must not re-render them with the app shell.
  return memo(function DeferredComponent(props: Props) {
    return <Suspense fallback={<RegionLoader label="Loading" />}>
      <Component {...props} />
    </Suspense>;
  });
}
