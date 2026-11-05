import type { ChatMsg, JsonValue } from "@codeterm/plugin-sdk";

export const object = (value:unknown):Record<string,JsonValue> =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string,JsonValue> : {};
export const text = (value:unknown):string|undefined => typeof value === "string" && value.length > 0 ? value : undefined;

// WHY: hosts decode frames into maps whose key order varies by platform, so provider JSON
// is rendered with sorted keys to keep tool text identical everywhere.
export const canonicalJson = (value:JsonValue):string => JSON.stringify(value, (_key, item:unknown) =>
  item !== null && typeof item === "object" && !Array.isArray(item)
    ? Object.fromEntries(Object.keys(item).sort().map((key) => [key, (item as Record<string,unknown>)[key]]))
    : item);

function diff(content:Record<string,JsonValue>):JsonValue|undefined {
  if (typeof content.path !== "string" || typeof content.newText !== "string"
    || content.oldText !== null && typeof content.oldText !== "string") return undefined;
  const lines = (value:string):string[] => value ? value.replace(/\n$/, "").split("\n") : [];
  const oldLines = lines(typeof content.oldText === "string" ? content.oldText : "");
  const newLines = lines(content.newText);
  return {path:content.path,kind:content.oldText === null ? "add" : "modify",
    additions:newLines.length,deletions:oldLines.length,
    hunks:[{oldStart:oldLines.length ? 1 : 0,oldLines:oldLines.length,
      newStart:newLines.length ? 1 : 0,newLines:newLines.length,
      lines:[...oldLines.map(line=>`-${line}`),...newLines.map(line=>`+${line}`)]}]};
}

export function reduceTool(update:Record<string,JsonValue>, previous:ChatMsg|undefined, seq:number):ChatMsg|undefined {
  const id = text(update.toolCallId);
  if (!id) return undefined;
  const row:ChatMsg = {...previous,id,seq:previous?.seq ?? seq,type:"tool_call",content:"",toolId:id,collapsed:true};
  if (typeof update.title === "string") { row.toolTitle=update.title; row.toolName=update.title; }
  if (typeof update.kind === "string") {
    const kinds:Record<string,string>={execute:"command",edit:"edit",read:"read",search:"search"};
    row.toolKind=Object.prototype.hasOwnProperty.call(kinds,update.kind) ? kinds[update.kind] : "generic";
  }
  if (update.rawInput !== undefined) {
    row.toolInput=update.rawInput;
    row.toolArgs=typeof update.rawInput === "string" ? update.rawInput : canonicalJson(update.rawInput);
  }
  const statuses = {pending:"running",in_progress:"running",completed:"completed",failed:"failed",cancelled:"interrupted"} as const;
  const status = Object.prototype.hasOwnProperty.call(statuses,String(update.status))
    ? statuses[String(update.status) as keyof typeof statuses] : undefined;
  if (status) { row.toolStatus=status; row.toolError=status === "failed"; }
  if (update.rawOutput !== undefined) row.toolResult=typeof update.rawOutput === "string" ? update.rawOutput : canonicalJson(update.rawOutput);
  if (Array.isArray(update.content)) {
    const results:string[]=[];
    const diffs:JsonValue[]=[];
    for (const raw of update.content) {
      const part=object(raw);
      const block=object(part.content);
      if (part.type === "content" && block.type === "text" && typeof block.text === "string") results.push(block.text);
      if (part.type === "diff") { const mapped=diff(part); if (mapped) diffs.push(mapped); }
    }
    if (results.length) row.toolResult=results.join("\n");
    if (diffs.length) row.fileDiffs=diffs;
  }
  return row;
}
