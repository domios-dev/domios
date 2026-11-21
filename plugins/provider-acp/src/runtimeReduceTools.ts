import type { ChatMsg, JsonValue } from "@codeterm/plugin-sdk";
import { utf8Length } from "./runtimeReduceAccounting";

export const object = (value:unknown):Record<string,JsonValue> =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string,JsonValue> : {};
export const text = (value:unknown):string|undefined => typeof value === "string" && value.length > 0 ? value : undefined;
export const MAX_TOOL_CALL_ID_BYTES = 4096;
export function validToolCallId(value:unknown):string|undefined {
  const id=text(value);
  if (!id || id.includes("\0") || utf8Length(JSON.stringify(id))-2 > MAX_TOOL_CALL_ID_BYTES) return undefined;
  return id;
}

// WHY: hosts decode frames into maps whose key order varies by platform, so provider JSON
// is rendered with sorted keys to keep tool text identical everywhere.
export const canonicalJson = (value:JsonValue):string => JSON.stringify(value, (_key, item:unknown) =>
  item !== null && typeof item === "object" && !Array.isArray(item)
    ? Object.fromEntries(Object.keys(item).sort().map((key) => [key, (item as Record<string,unknown>)[key]]))
    : item);

const MAX_DIFF_LINES=4096;
const MAX_DIFF_CELLS=40000;
const MAX_CONTENT_PARTS=64;

function lines(value:string):string[] {
  if (!value) return [];
  const result=value.split(/\r?\n/);
  if (result.at(-1) === "") result.pop();
  return result;
}

function diff(content:Record<string,JsonValue>):{value:JsonValue;omitted:boolean}|undefined {
  if (typeof content.path !== "string" || !content.path.trim() || typeof content.newText !== "string"
    || content.oldText !== null && typeof content.oldText !== "string") return undefined;
  const oldLines=content.oldText === null ? [] : lines(content.oldText);
  const newLines=lines(content.newText);
  if (oldLines.length > MAX_DIFF_LINES || newLines.length > MAX_DIFF_LINES)
    return {value:{},omitted:true};

  let prefix=0;
  while (prefix<oldLines.length && prefix<newLines.length && oldLines[prefix] === newLines[prefix]) prefix++;
  let oldEnd=oldLines.length,newEnd=newLines.length;
  while (oldEnd>prefix && newEnd>prefix && oldLines[oldEnd-1] === newLines[newEnd-1]) {oldEnd--;newEnd--;}
  const oldCount=oldEnd-prefix,newCount=newEnd-prefix;
  if (!oldCount && !newCount) {
    if (content.oldText === null) return {value:{path:content.path,kind:"add",additions:0,deletions:0,
      hunks:[{oldStart:0,oldLines:0,newStart:0,newLines:0,lines:[]}]},omitted:false};
    return undefined;
  }

  const columns=newCount+1;
  const cells=(oldCount+1)*columns;
  if (cells > MAX_DIFF_CELLS) return {value:{},omitted:true};
  const lcs=new Uint32Array(cells);
  for (let oldIndex=oldCount-1;oldIndex>=0;oldIndex--) {
    for (let newIndex=newCount-1;newIndex>=0;newIndex--) {
      const at=oldIndex*columns+newIndex;
      lcs[at]=oldLines[prefix+oldIndex] === newLines[prefix+newIndex]
        ? lcs[(oldIndex+1)*columns+newIndex+1]+1
        : Math.max(lcs[(oldIndex+1)*columns+newIndex],lcs[at+1]);
    }
  }

  const changed:string[]=[];
  let oldIndex=0,newIndex=0;
  while (oldIndex<oldCount || newIndex<newCount) {
    if (oldIndex<oldCount && newIndex<newCount && oldLines[prefix+oldIndex] === newLines[prefix+newIndex]) {
      changed.push(` ${oldLines[prefix+oldIndex]}`);oldIndex++;newIndex++;
    } else if (newIndex<newCount && (oldIndex===oldCount
      || lcs[oldIndex*columns+newIndex+1] > lcs[(oldIndex+1)*columns+newIndex])) {
      changed.push(`+${newLines[prefix+newIndex]}`);newIndex++;
    } else {
      changed.push(`-${oldLines[prefix+oldIndex]}`);oldIndex++;
    }
  }
  return {value:{path:content.path,kind:content.oldText === null ? "add" : "modify",
    additions:changed.filter(line=>line.startsWith("+")).length,
    deletions:changed.filter(line=>line.startsWith("-")).length,
    hunks:[{oldStart:oldCount ? prefix+1 : prefix,oldLines:oldCount,
      newStart:newCount ? prefix+1 : prefix,newLines:newCount,lines:changed}]},omitted:false};
}

function locationSummary(value:JsonValue):string {
  if (!Array.isArray(value)) return "";
  const locations:string[]=[];
  let count=0;
  for (const raw of value) {
    const location=object(raw);
    if (typeof location.path !== "string" || !location.path.trim()) continue;
    count++;
    if (locations.length<8) locations.push(typeof location.line === "number" && Number.isSafeInteger(location.line) && location.line > 0
      ? `${location.path}:${location.line}` : location.path);
  }
  if (count>locations.length) locations.push(`+${count-locations.length} more locations`);
  return locations.join(", ");
}

export function reduceTool(update:Record<string,JsonValue>, previous:ChatMsg|undefined, seq:number):ChatMsg|undefined {
  const id = validToolCallId(update.toolCallId);
  if (!id) return undefined;
  const row:ChatMsg = {...previous,id,seq:previous?.seq ?? seq,type:"tool_call",content:"",toolId:id,collapsed:true};
  if (typeof update.title === "string") { row.toolTitle=update.title;row.toolName=update.title; }
  if (typeof update.name === "string") row.toolName=update.name;
  if (typeof update.kind === "string") {
    const kinds:Record<string,string>={execute:"command",edit:"edit",read:"read",search:"search"};
    row.toolKind=Object.prototype.hasOwnProperty.call(kinds,update.kind) ? kinds[update.kind] : "generic";
  }
  if (update.rawInput !== undefined && update.rawInput !== null) {
    row.toolInput=update.rawInput as Record<string,unknown>;
    row.toolArgs=typeof update.rawInput === "string" ? update.rawInput : canonicalJson(update.rawInput);
  }
  const statuses = {pending:"running",in_progress:"running",completed:"completed",failed:"failed",cancelled:"interrupted"} as const;
  const status = Object.prototype.hasOwnProperty.call(statuses,String(update.status))
    ? statuses[String(update.status) as keyof typeof statuses] : undefined;
  if (status) { row.toolStatus=status; row.toolError=status === "failed"; }
  const hasRawOutput=update.rawOutput !== undefined && update.rawOutput !== null;
  if (hasRawOutput)
    row.toolResult=typeof update.rawOutput === "string" ? update.rawOutput : canonicalJson(update.rawOutput);
  if (Array.isArray(update.locations)) row.toolSummary=locationSummary(update.locations);
  if (Array.isArray(update.content)) {
    const results:string[]=[];
    const diffs:JsonValue[]=[];
    let omitted=update.content.length > MAX_CONTENT_PARTS;
    for (const raw of update.content.slice(0,MAX_CONTENT_PARTS)) {
      const part=object(raw);
      const block=object(part.content);
      if (part.type === "content" && block.type === "text" && typeof block.text === "string") results.push(block.text);
      if (part.type === "diff") {
        const mapped=diff(part);
        if (mapped?.omitted) omitted=true;
        else if (mapped) diffs.push(mapped.value);
      }
    }
    const explicitEmpty=update.content.length === 0 && previous !== undefined;
    if (results.length || omitted) row.toolResult=[...results,...(omitted ? ["ACP tool detail was omitted because the bounded diff or content limit was reached."] : [])].join("\n");
    else if (!hasRawOutput && (explicitEmpty || previous?.toolResult !== undefined)) row.toolResult="";
    if (diffs.length) row.fileDiffs=diffs;
    else if (explicitEmpty || previous?.fileDiffs !== undefined) row.fileDiffs=[];
  }
  return row;
}
