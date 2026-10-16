import {randomUUID} from 'node:crypto';

export function fixtureRecord({session,cwd,index,parent=null,run}) {
  if (!session || !cwd || !run || !Number.isSafeInteger(index) || index < 0) throw Error('Invalid synthetic record identity');
  const marker = `MEMSOAK_${run}_${index}`;
  const code = Array.from({length:18},(_,n)=>`const item${n} = {index: ${index}, value: "synthetic-${n}"};`).join('\n');
  const table = Array.from({length:8},(_,n)=>`| ${n} | fixture-${index}-${n} | complete |`).join('\n');
  const content = `${marker}\n\nSynthetic streaming response ${index}. No live agent was called.\n\n\`\`\`typescript\n${code}\n\`\`\`\n\n| Row | Value | Status |\n|---|---|---|\n${table}\n\n${'Bounded synthetic transcript content. '.repeat(20)}`;
  const uuid = randomUUID();
  return {marker,uuid,line:JSON.stringify({type:'assistant',uuid,parentUuid:parent,sessionId:session,cwd,timestamp:new Date().toISOString(),message:{id:uuid,type:'message',role:'assistant',model:'synthetic-memory-fixture',content:[{type:'text',text:content}],stop_reason:'end_turn',usage:{input_tokens:1,output_tokens:1}}})+'\n'};
}

export function growth(first,last) {
  const previous = new Map(first.map(row=>[`${row.pid}:${row.created}`,row]));
  return last.flatMap(row=>{const old=previous.get(`${row.pid}:${row.created}`);return old?[{pid:row.pid,name:row.name,before:old.privateBytes,after:row.privateBytes,delta:row.privateBytes-old.privateBytes}]:[];});
}
