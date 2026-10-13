import type {RuntimeContext, RuntimeActiveTurn} from "@codeterm/plugin-sdk";

export function interruptTargets(ctx:RuntimeContext, root:string, active?:string): RuntimeActiveTurn[] {
  const turns=[...ctx.activeTurns];
  if (active && !turns.some(turn=>turn.threadId === root && turn.turnId === active)) turns.push({threadId:root,turnId:active});
  const depth=(thread:string):number=>{
    let current=thread;let count=0;const seen=new Set<string>();
    while(current !== root){
      if(seen.has(current))return -1;seen.add(current);
      const parent=turns.find(turn=>turn.threadId === current)?.parentThreadId;
      if(!parent)return -1;current=parent;count++;
    }
    return count;
  };
  return turns.map(turn=>({turn,depth:depth(turn.threadId)})).filter(entry=>entry.depth >= 0)
    .sort((a,b)=>b.depth-a.depth || a.turn.threadId.localeCompare(b.turn.threadId)).map(entry=>entry.turn);
}
