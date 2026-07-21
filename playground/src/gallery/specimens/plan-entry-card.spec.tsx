import { PlanEntryCard } from "../../ds/PlanEntryCard";
import type { Specimen } from "../types";
export default { id:"plan-entry-card", title:"Plan entry card", category:"Sidebar", where:"src/components/Sidebar/WorkspacePlanEntry.tsx", states:[{id:"progress",label:"66 / 70",props:{}},{id:"tick",label:"67 / 70",props:{completed:67}},{id:"selected",label:"Selected",props:{selected:true}},{id:"complete",label:"Complete",props:{completed:70}}], Component:PlanEntryCard, notes:"Changing completed remounts the ring and counter for one liquid beat." } satisfies Specimen;
