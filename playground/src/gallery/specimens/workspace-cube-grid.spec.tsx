import { WorkspaceCubeGrid } from "../../ds/WorkspaceCubeGrid";
import type { Specimen } from "../types";
const workspaces = [{id:"aa",initials:"AA",name:"Atlas",selected:true},{id:"am",initials:"AM",name:"Amber",working:true},{id:"ct",initials:"CT",name:"CodeTerm"},{id:"kp",initials:"KP",name:"Kite"}];

export default { id:"workspace-cube-grid", title:"Workspace cube grid", category:"Sidebar", where:"src/components/Sidebar/WorkspaceCardGrid.tsx", states:[{id:"mixed",label:"Selected + working",props:{workspaces}},{id:"unselected",label:"Unselected",props:{workspaces:[{id:"aa",initials:"AA",name:"Atlas"},{id:"am",initials:"AM",name:"Amber"},{id:"ct",initials:"CT",name:"CodeTerm"},{id:"kp",initials:"KP",name:"Kite"}]}},{id:"working",label:"Working",props:{workspaces:[{id:"aa",initials:"AA",name:"Atlas",working:true},{id:"am",initials:"AM",name:"Amber",working:true}]}}], Component:WorkspaceCubeGrid } satisfies Specimen;
