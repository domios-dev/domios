import { useCallback, useState, type ComponentProps } from "react";
import { BlueprintView } from "../Automation/blueprint/BlueprintView";
import { useViewerNavigation } from "../../hooks/useViewerNavigation";
import { blueprintNavigation, restoreBlueprintNavigation, type BlueprintNavigation } from "../../lib/blueprintNavigation";

type Props = ComponentProps<typeof BlueprintView> & { navigationScope: string };

export function PersistedBlueprintView({ navigationScope, ...props }: Props) {
  const [saved, setSaved] = useViewerNavigation<BlueprintNavigation>(`blueprint:${navigationScope}`, { runId: props.initialRunId ?? null, trail: [], selected: null }, blueprintNavigation, props.initialRunId === undefined);
  const [initialRoute] = useState(() => restoreBlueprintNavigation(saved, props.pipeline));
  const onRouteChange = useCallback((route: BlueprintNavigation) => setSaved(route), [setSaved]);
  return <BlueprintView {...props} initialRoute={initialRoute} onRouteChange={onRouteChange} />;
}
