import type {ChatMsg} from "@codeterm/plugin-sdk";
import {fail, list, object, text} from "./runtimeProtocol";

export function contentId(message:string, kind:"text"|"reasoning", ordinal:number):string {return `${message}:${kind}:${ordinal}`;}
export function toolId(message:string, id:string):string {return `${message}:tool:${id}`;}
export function toolContent(raw:unknown):string {return list(raw).map(object).filter(part=>part.type === "text" && typeof part.text === "string").map(part=>String(part.text)).join("\n");}
export function v2Content(message:string, raw:unknown, ordinal:number, seq:number, ts?:number):ChatMsg|undefined {
  const part=object(raw), type=part.type;
  const timestamp=ts !== undefined ? {timestamp:new Date(ts).toISOString()} : {};
  if (type === "text" || type === "reasoning") return {id:contentId(message,type,ordinal),seq,type:type === "text" ? "assistant" : "thinking",content:String(part.text ?? ""),...timestamp};
  if (type !== "tool" || !text(part.id) || !text(part.name)) return undefined;
  const state=object(part.state), time=object(part.time), status=String(state.status);
  return {id:toolId(message,String(part.id)),seq,type:"tool_call",content:"",toolName:String(part.name),toolId:String(part.id),toolInput:object(state.input),toolStatus:status,
    ...(state.content ? {toolResult:toolContent(state.content)} : {}),...(state.error ? {toolResult:String(object(state.error).message ?? "Tool failed"),toolError:true} : {}),
    ...(typeof time.ran === "number" ? {toolStartedAtMs:time.ran} : {}),...(typeof time.completed === "number" ? {toolCompletedAtMs:time.completed} : {}),...timestamp};
}
export function v2History(raw:unknown):ChatMsg[] {
  const entries=list(raw), rows:ChatMsg[]=[];
  // Pages arrive newest first, but the host prepends each page in transcript order.
  for (let index=entries.length-1;index>=0;index--) {
    const message=object(entries[index]), id=text(message.id), created=object(message.time).created;
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
