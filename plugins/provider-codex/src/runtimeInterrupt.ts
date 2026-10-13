import type {RuntimeContext, RuntimeActiveTurn} from "@codeterm/plugin-sdk";

export function interruptTargets(ctx:RuntimeContext, root:string): RuntimeActiveTurn[] {
  const turns=[...ctx.activeTurns];
  const depth=(thread:string):number=>{
    let current=thread;let count=0;const seen=new Set<string>();
    while(current !== root){
      if(seen.has(current))return -1;seen.add(current);
      const parent=ctx.threadParents.find(parent=>parent.threadId === current)?.parentThreadId;
      if(!parent)return -1;current=parent;count++;
    }
    return count;
  };
  return turns.map(turn=>({turn,depth:depth(turn.threadId)})).filter(entry=>entry.depth >= 0)
    .sort((a,b)=>b.depth-a.depth || a.turn.threadId.localeCompare(b.turn.threadId)).map(entry=>entry.turn);
}
