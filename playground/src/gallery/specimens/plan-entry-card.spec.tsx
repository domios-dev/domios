import { PlanEntryCard } from "../../ds/PlanEntryCard";
import type { Specimen } from "../types";
const base = { title: "General 1.7.2", subtitle: "General 1.7.2 plan", completed: 66, total: 70 };

export default { id:"plan-entry-card", title:"Plan entry card", category:"Sidebar", where:"src/components/Sidebar/WorkspacePlanEntry.tsx", states:[{id:"progress",label:"66 / 70",props:base},{id:"working",label:"Working",props:{...base,working:true}},{id:"tick",label:"67 / 70",props:{...base,completed:67}},{id:"selected",label:"Selected",props:{...base,selected:true}},{id:"complete",label:"Complete",props:{...base,completed:70}}], Component:PlanEntryCard, notes:"Changing completed remounts the ring and counter for one liquid beat." } satisfies Specimen;
