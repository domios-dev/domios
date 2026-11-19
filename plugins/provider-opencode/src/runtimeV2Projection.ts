import type {ChatMsg} from "@codeterm/plugin-sdk";
import {canonicalToolFields, toolFileDiffs, type OpencodePart} from "./helpers";
import {fail, list, object, text} from "./runtimeProtocol";

export function contentId(message:string, kind:"text"|"reasoning", ordinal:number):string {return `${message}:${kind}:${ordinal}`;}
export function toolId(message:string, id:string):string {return `${message}:tool:${id}`;}
export function toolContent(raw:unknown):string {return list(raw).map(object).filter(part=>part.type === "text" && typeof part.text === "string").map(part=>String(part.text)).join("\n");}
function toolInput(raw:unknown):Record<string,unknown> {
  if (raw && typeof raw === "object" && !Array.isArray(raw)) return raw as Record<string,unknown>;
  if (typeof raw !== "string") return {};
  try { return object(JSON.parse(raw)); }
  catch (_) { return {}; }
}
function errorText(raw:unknown):string {
  if (typeof raw === "string") return raw;
  const error=object(raw), data=object(error.data);
  return text(error.message) || text(data.message) || text(error.type) || "Tool failed";
}
export function v2Content(message:string, raw:unknown, ordinal:number, seq:number, ts?:number):ChatMsg|undefined {
  const part=object(raw), type=part.type;
  const timestamp=ts !== undefined ? {timestamp:new Date(ts).toISOString()} : {};
  if (type === "text" || type === "reasoning") return {id:contentId(message,type,ordinal),seq,type:type === "text" ? "assistant" : "thinking",content:String(part.text ?? ""),...timestamp};
  if (type !== "tool" || !text(part.id) || !text(part.name)) return undefined;
  const name=String(part.name), id=String(part.id), state=object(part.state), time=object(part.time);
  const input=toolInput(state.input), metadata={...object(part.metadata),...object(state.metadata)};
  const error=object(state.error), aborted=state.status === "error" && error.type === "aborted";
  const timedOut=!aborted && metadata.timeout === true;
  const failed=!aborted && (state.status === "error" || timedOut);
  const status=aborted ? "interrupted" : failed ? "failed" : state.status === "streaming" ? "running" : String(state.status);
  const canonical=canonicalToolFields(name,input);
  const partForDiffs:OpencodePart={type:"tool",tool:name,state:{...state,input,metadata,status:String(state.status)}};
  const diffs=toolFileDiffs(partForDiffs);
  const result=state.error ? errorText(state.error) : toolContent(state.content);
  return {id:toolId(message,id),seq,type:"tool_call",content:"",toolName:name,toolId:id,toolKind:canonical.kind,toolSummary:canonical.summary,
    toolDetail:canonical.detail,toolEdits:canonical.edits,fileDiffs:diffs,toolInput:input,toolStatus:status,
    ...(result ? {toolResult:result} : {}),...(aborted ? {toolInterrupted:true} : failed ? {toolError:true} : state.status === "completed" ? {toolError:false} : {}),
    ...(typeof time.ran === "number" ? {toolStartedAtMs:time.ran} : {}),...(typeof time.completed === "number" ? {toolCompletedAtMs:time.completed} : {}),...timestamp};
}
export function v2History(raw:unknown, order:"asc"|"desc"="desc"):ChatMsg[] {
  const entries=list(raw), rows:ChatMsg[]=[];
  // Every page is projected in transcript order, including newest-first reads.
  const chronological=order === "desc" ? [...entries].reverse() : entries;
  for (const entry of chronological) {
    const message=object(entry), id=text(message.id), created=object(message.time).created;
    if (!id) fail("invalid_schema","OpenCode history message is missing its identity");
    const ts=typeof created === "number" ? created : undefined;
    if (message.type === "user") {
      rows.push({id,seq:rows.length+1,type:"user",content:String(message.text ?? ""),...(ts !== undefined ? {timestamp:new Date(ts).toISOString()} : {}),attachments:list(message.files).map((raw,index)=>{
        const file=object(raw);return {id:`${id}:file:${index}`,mimeType:String(file.mime ?? "application/octet-stream"),url:String(file.uri)};
      })});
    } else if (message.type === "assistant") {
      const content=list(message.content);
      if (content.length > 512) fail("size_limit","OpenCode history content exceeds its bound");
      // Stream ordinals count text and reasoning separately within each message.
      const ordinals={text:0,reasoning:0};
      for (const part of content.map(object)) {
        const kind=part.type;
        const ordinal=kind === "text" || kind === "reasoning" ? ordinals[kind]++ : 0;
        const row=v2Content(id,part,ordinal,rows.length+1,ts);
        if (row) rows.push(row);
      }
    }
  }
  return rows;
}
