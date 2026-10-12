import { useState } from "react";
import { initialProviderLogoState, providerLogoPresentation } from "./providerLogoModel";

export function useProviderLogo(iconSvg?: string | null) {
  const [state, setState] = useState(() => initialProviderLogoState(iconSvg));
  return {
    presentation: providerLogoPresentation(state),
    onLoad: () => setState((previous) => previous === "failed" ? previous : "available"),
    onError: () => setState("failed"),
  };
}
