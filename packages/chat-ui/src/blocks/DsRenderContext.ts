import { createContext } from "react";
export interface DsRenderScope { authority: string; trusted: boolean; streaming?: boolean; cancelled?: boolean; proseOnly?: boolean }
export const DsRenderContext = createContext<DsRenderScope>({ authority: "standalone", trusted: true });
