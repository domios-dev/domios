import type {ChatMsg} from "@codeterm/plugin-sdk";
import {partToChatMsg, type OpencodePart} from "./helpers";
import {fail, list, object, text} from "./runtimeProtocol";

export function partRow(raw:unknown, info:Record<string,unknown>, seq:number):ChatMsg|undefined {
  const part = object(raw), id = text(part.id), role = info.role;
  if (!id || (role !== "user" && role !== "assistant")) return undefined;
  const created = Number(object(info.time).created);
  const mapped = partToChatMsg(part as OpencodePart,role,id,Number.isSafeInteger(created) && created >= 0 ? created : 0);
  let row:ChatMsg|undefined = mapped ? {...Object.fromEntries(Object.entries(object(mapped)).filter(([,value])=>value !== undefined)),id,seq,type:String(object(mapped).type),content:String(object(mapped).content ?? "")} : undefined;
  if (!row && (part.type === "text" || part.type === "reasoning")) row = {id,seq,type:part.type === "reasoning" ? "thinking" : role,content:""};
  if (part.type === "file" && typeof part.url === "string" && typeof part.mime === "string") {
    row = {id,seq,type:role,content:"",attachments:[{id,mimeType:part.mime,url:part.url}]};
  }
  if (!row) return undefined;
  if (part.type === "tool") {
    const state = object(part.state), time = object(state.time);
    const status=text(state.status);
    if (status) row.toolStatus=status;
    if (typeof time.start === "number") row.toolStartedAtMs = time.start;
    if (typeof time.end === "number") row.toolCompletedAtMs = time.end;
    if (state.status === "error" && typeof state.error === "string") {row.toolError=true;row.toolResult=state.error;}
  }
  return row;
}
export function historyRows(thread:string, body:unknown, ordinal=0):ChatMsg[] {
  if (!Array.isArray(body) || body.length > 100) fail("invalid_schema","OpenCode history page exceeds its declared bound");
  const rows:ChatMsg[] = [];
  for (const raw of body) {
    const message = object(raw), info = object(message.info);
    if (info.sessionID !== thread || !text(info.id)) fail("binding_mismatch","History includes a foreign session message");
    const parts = list(message.parts);
    if (parts.length > 512) fail("size_limit","OpenCode history part capacity exceeded");
    const projected:ChatMsg[] = [];
    for (const part of parts) {
      if (object(part).sessionID !== thread || object(part).messageID !== info.id) fail("binding_mismatch","History part ownership differs");
      const row = partRow(part,info,ordinal+rows.length+1);
      if (row) projected.push(row);
    }
    if (info.role === "user") rows.push({id:String(info.id),seq:ordinal+rows.length+1,type:"user",content:projected.map(row => row.content).filter(Boolean).join("\n"),attachments:projected.flatMap(row => row.attachments ?? [])});
    else rows.push(...projected);
  }
  return rows;
}
