import type { ReactNode } from "react";
import { BrandState } from "./BrandState";
import { brandConnectionModel } from "./brandStateModel";
import type { BrandStateModel } from "./brandStateModel";
import { useServerConnectionState } from "./useServerConnectionState";

export function ServerConnectionLayerView({
  children,
  className = "",
  contentClassName = "",
  model,
  "data-testid": testId,
  "data-shell-viewport": viewport,
}: {
  children: ReactNode;
  model: BrandStateModel;
  className?: string;
  contentClassName?: string;
  "data-testid"?: string;
  "data-shell-viewport"?: string;
}) {
  return (
    <div className={`brand-connection-layer ${className}`}>
      <div className={contentClassName} inert={model.kind === "reconnecting" || undefined} data-testid={testId} data-shell-viewport={viewport}>
        {children}
      </div>
      <BrandState model={model} />
    </div>
  );
}

export function ServerConnectionLayer(props: Omit<Parameters<typeof ServerConnectionLayerView>[0], "model">) {
  const connection = useServerConnectionState();
  return <ServerConnectionLayerView {...props} model={brandConnectionModel(connection, Date.now())} />;
}
