import type { ChatMsg, JsonValue } from "@codeterm/plugin-sdk";
import { utf8Length } from "./runtimeReduceAccounting";
import { imageTooLarge, IMAGE_DATA_BUDGET, inlineImage } from "./runtimeOutput";

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
const MAX_LOCATIONS=8;

export interface ToolLocationItem {
  path: string;
  line?: number;
}

export function fileUriToPath(uri:string):string|undefined {
  if (!uri.startsWith("file://")) return undefined;
  let rest=uri.slice(7);
  if (rest.startsWith("localhost/")) rest=rest.slice(9);
  const hash=rest.indexOf("#");
  if (hash >= 0) rest=rest.slice(0,hash);
  const query=rest.indexOf("?");
  if (query >= 0) rest=rest.slice(0,query);
  try { rest=decodeURIComponent(rest); } catch { /* keep raw percent sequence */ }
  if (/^\/[A-Za-z]:[\\/]/.test(rest)) rest=rest.slice(1);
  return rest.trim() ? rest : undefined;
}

export function pathToFileUri(rawPath:string):string {
  const normalized=rawPath.replace(/\\/g,"/");
  const prefixed=/^[A-Za-z]:\//.test(normalized) ? `/${normalized}` : normalized.startsWith("/") ? normalized : `/${normalized}`;
  return `file://${encodeURI(prefixed).replace(/#/g,"%23").replace(/\?/g,"%3F")}`;
}

function lines(value:string):string[] {
  if (!value) return [];
  const result=value.split(/\r?\n/);
  if (result[result.length-1] === "") result.pop();
  return result;
}

export function diff(content:Record<string,JsonValue>):{value:JsonValue;omitted:boolean}|undefined {
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

export function extractToolLocations(value:JsonValue):ToolLocationItem[] {
  if (!Array.isArray(value)) return [];
  const result:ToolLocationItem[]=[];
  const seen=new Set<string>();
  for (const raw of value) {
    const location=object(raw);
    if (typeof location.path !== "string" || !location.path.trim()) continue;
    const line=typeof location.line === "number" && Number.isSafeInteger(location.line) && location.line > 0 ? location.line : undefined;
    const key=line !== undefined ? `${location.path}:${line}` : location.path;
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(line !== undefined ? {path:location.path,line} : {path:location.path});
    if (result.length >= MAX_LOCATIONS) break;
  }
  return result;
}

function locationSummary(value:JsonValue):string {
  if (!Array.isArray(value)) return "";
  const locations:string[]=[];
  let count=0;
  for (const raw of value) {
    const location=object(raw);
    if (typeof location.path !== "string" || !location.path.trim()) continue;
    count++;
    if (locations.length<MAX_LOCATIONS) locations.push(typeof location.line === "number" && Number.isSafeInteger(location.line) && location.line > 0
      ? `${location.path}:${location.line}` : location.path);
  }
  if (count>locations.length) locations.push(`+${count-locations.length} more locations`);
  return locations.join(", ");
}

function pushLocation(list:ToolLocationItem[], path:string, line?:number):void {
  if (list.length >= MAX_LOCATIONS || !path.trim()) return;
  if (list.some(entry=>entry.path === path && entry.line === line)) return;
  list.push(line !== undefined ? {path,line} : {path});
}

const TOOL_KINDS:Record<string,string> = {
  execute:"command",
  edit:"edit",
  delete:"edit",
  move:"edit",
  read:"read",
  search:"search",
  fetch:"fetch",
  think:"generic",
  switch_mode:"generic",
};

export function reduceTool(update:Record<string,JsonValue>, previous:ChatMsg|undefined, seq:number, imageNotices:string[]=[], admitImage:(bytes:number)=>boolean=()=>true):ChatMsg|undefined {
  const id = validToolCallId(update.toolCallId);
  if (!id) return undefined;
  const row:ChatMsg = {...previous,id,seq:previous?.seq ?? seq,type:"tool_call",content:"",toolId:id,collapsed:true};
  if (typeof update.title === "string") { row.toolTitle=update.title;row.toolName=update.title; }
  if (typeof update.name === "string") row.toolName=update.name;
  if (typeof update.kind === "string") {
    row.toolKind=Object.prototype.hasOwnProperty.call(TOOL_KINDS,update.kind) ? TOOL_KINDS[update.kind] : "generic";
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
  if (hasRawOutput && update.rawOutput !== "")
    row.toolResult=typeof update.rawOutput === "string" ? update.rawOutput : canonicalJson(update.rawOutput);
  let locations:ToolLocationItem[]|undefined;
  if (Array.isArray(update.locations)) {
    row.toolSummary=locationSummary(update.locations);
    locations=extractToolLocations(update.locations);
  }
  if (Array.isArray(update.content)) {
    const results:string[]=[];
    const diffs:JsonValue[]=[];
    const images:NonNullable<ChatMsg["attachments"]>=[];
    let imageBytes=0;
    const addImage=(mimeType:unknown,data:unknown):boolean=>{
      const image=inlineImage(mimeType,data);
      if (!image) return false;
      if (imageBytes+image.data.length > IMAGE_DATA_BUDGET || !admitImage(image.data.length))
        imageNotices.push(imageTooLarge(image.data));
      else {
        imageBytes+=image.data.length;
        images.push({id:`image-${images.length+1}`,...image});
      }
      return true;
    };
    let omitted=update.content.length > MAX_CONTENT_PARTS;
    for (const raw of update.content.slice(0,MAX_CONTENT_PARTS)) {
      const part=object(raw);
      if (part.type === "content") {
        const block=object(part.content);
        if (block.type === "text" && typeof block.text === "string") results.push(block.text);
        else if (block.type === "image") {
          if (!addImage(block.mimeType,block.data)) omitted=true;
        } else if (block.type === "resource_link") {
          const uri=text(block.uri);
          if (uri) {
            const label=text(block.title) ?? text(block.name);
            const filePath=fileUriToPath(uri);
            if (filePath) {
              if (!locations) locations = [];
              pushLocation(locations,filePath);
            }
            results.push(label ? `${label} (${filePath ?? uri})` : (filePath ?? uri));
          }
        } else if (block.type === "resource") {
          const resource=object(block.resource);
          const uri=text(resource.uri);
          const filePath=uri ? fileUriToPath(uri) : undefined;
          if (filePath) {
            if (!locations) locations = [];
            pushLocation(locations,filePath);
          }
          if (typeof resource.text === "string" && resource.text.length) results.push(resource.text);
          else if (typeof resource.blob === "string") {
            if (!addImage(resource.mimeType,resource.blob) && uri) results.push(filePath ?? uri);
          } else if (uri) results.push(filePath ?? uri);
        } else if (block.type === "audio") {
          const mime=text(block.mimeType) ?? "audio";
          results.push(`[Audio content (${mime})]`);
        }
      } else if (part.type === "terminal") {
        const terminalId=validToolCallId(part.terminalId);
        if (terminalId) {
          row.toolBackgroundTaskId=terminalId;
          if (!row.toolKind) row.toolKind="command";
        }
        if (typeof part.output === "string" && part.output.length) results.push(part.output);
        const exitStatus=object(part.exitStatus);
        const exitCode=typeof exitStatus.exitCode === "number" && Number.isSafeInteger(exitStatus.exitCode)
          ? exitStatus.exitCode
          : typeof part.exitCode === "number" && Number.isSafeInteger(part.exitCode) ? part.exitCode : undefined;
        if (exitCode !== undefined) row.toolExitCode=exitCode;
      } else if (part.type === "diff") {
        const mapped=diff(part);
        if (mapped?.omitted) omitted=true;
        else if (mapped) diffs.push(mapped.value);
      }
    }
    const explicitEmpty=update.content.length === 0 && previous !== undefined;
    if (results.length) row.toolResult=results.join("\n");
    else if (!hasRawOutput && explicitEmpty) row.toolResult="";
    if (omitted) row.toolDetail=[row.toolDetail,"ACP tool detail was omitted because the bounded diff or content limit was reached."].filter(Boolean).join("\n");
    if (images.length) row.attachments=images;
    if (diffs.length) row.fileDiffs=diffs;
    else if (explicitEmpty) row.fileDiffs=[];
  }
  if (locations !== undefined) {
    if (locations.length) (row as Record<string,unknown>).toolLocations=locations;
    else if (previous && (previous as Record<string,unknown>).toolLocations !== undefined) (row as Record<string,unknown>).toolLocations=[];
  }
  return row;
}
