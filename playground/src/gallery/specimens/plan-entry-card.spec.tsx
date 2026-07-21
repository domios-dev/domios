import type { ComponentType } from "react";
import { PlanEntryCard } from "../../ds/PlanEntryCard";
type SpecimenDef = { id: string; title: string; where: string; states: Array<{ id: string; label: string; props: object }>; Component: ComponentType<any>; notes?: string };
export default { id:"plan-entry-card", title:"Plan entry card", where:"src/components/Sidebar/WorkspacePlanEntry.tsx", states:[{id:"progress",label:"66 / 70",props:{}},{id:"tick",label:"67 / 70",props:{completed:67}},{id:"selected",label:"Selected",props:{selected:true}},{id:"complete",label:"Complete",props:{completed:70}}], Component:PlanEntryCard, notes:"Changing completed remounts the ring and counter for one liquid beat." } satisfies SpecimenDef;
