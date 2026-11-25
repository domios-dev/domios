import { useState } from "react";
import type { AppConfig } from "../../lib/types";
import { deferredComponent } from "../shared/deferredComponent";
import type { AppearanceLabProjection, AppearanceLabSession } from "./AppearanceLabSession";

const AppearanceLabLayer = deferredComponent(async () => ({ default: (await import("./AppearanceLab")).AppearanceLabLayer }));

export interface AppearanceLabHostProps {
  config: AppConfig;
  session?: AppearanceLabSession;
  onProject(projection?: AppearanceLabProjection): void;
  onClose(returnToOrigin: boolean): void;
}

export function AppearanceLabHost(props: AppearanceLabHostProps) {
  const [mounted, setMounted] = useState(Boolean(props.session));
  if (props.session && !mounted) setMounted(true);
  return mounted ? <AppearanceLabLayer {...props} onExited={() => setMounted(false)} /> : null;
}
