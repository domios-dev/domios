interface Observation {id:string;state:"running"|"completed"|"killed"|"failed"}
function object(value:unknown):Record<string,unknown>|null {return value && typeof value==="object" && !Array.isArray(value) ? value as Record<string,unknown> : null;}
export function parseBackgroundWork(value:unknown):Observation[] {
  const row=object(value);
  if (!row) return [];
  const result=object(row.toolUseResult);
  const events:Observation[]=[];
  for (const [key,state] of [["backgroundTaskId","running"],["agentId",result?.status==="async_launched" || result?.status==="running" ? "running" : result?.status]] as const) {
    const id=result?.[key];
    if (typeof id==="string" && /^[A-Za-z0-9_-]{1,256}$/.test(id) && ["running","completed","failed","killed"].includes(String(state))) events.push({id,state:state as Observation["state"]});
  }
  const content=object(row.message)?.content;
  const prompt=object(row.attachment)?.prompt;
  const texts=typeof content==="string" ? [content] : Array.isArray(content) ? content.map(block=>object(block)?.text).filter((text):text is string=>typeof text==="string") : [];
  if (typeof prompt==="string") texts.push(prompt);
  for (const text of texts) {
    const notifications=text.match(/<task-notification>[\s\S]*?<\/task-notification>/g) || [];
    for (const block of notifications) {
      const id=/<task-id>([^<]+)<\/task-id>/.exec(block)?.[1].trim();
      const state=/<status>([^<]+)<\/status>/.exec(block)?.[1].trim();
      if (id && /^[A-Za-z0-9_-]{1,256}$/.test(id) && state && ["running","completed","killed","failed"].includes(state)) events.push({id,state:state as Observation["state"]});
    }
  }
  return events;
}
