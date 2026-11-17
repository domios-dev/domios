import { timestampMs,type NormalizedChatMessage } from "@codeterm/chat-core";
import type { AccountContinuationNotice } from "../../../gen/AccountContinuationNotice";

export function accountNoticeRows(notices:readonly AccountContinuationNotice[],provider:string|null|undefined,session:string|null):NormalizedChatMessage[] {
  return notices.filter(notice=>notice.provider===provider && notice.session===session).slice(-32).map(notice=>({id:`account-notice:${notice.id}`,type:"system",systemSubtype:"account_transition",content:notice.content,timestamp:String(notice.at_ms),toolInput:{verification:notice.verification,boundary:notice.boundary,resetsAtMs:notice.resets_at_ms}}));
}

export function mergeAccountNoticeRows(rows:readonly NormalizedChatMessage[],notices:readonly NormalizedChatMessage[]):NormalizedChatMessage[] {
  if(!notices.length)return rows as NormalizedChatMessage[];
  const ids=new Set(rows.map(row=>row.id));
  const pending=notices.filter(notice=>{if(ids.has(notice.id))return false;ids.add(notice.id);return true;}).slice().sort((left,right)=>(timestampMs(left.timestamp)??0)-(timestampMs(right.timestamp)??0));
  if(!pending.length)return rows as NormalizedChatMessage[];
  const merged:NormalizedChatMessage[]=[];
  let index=0;
  for(const row of rows){
    const at=timestampMs(row.timestamp);
    while(index<pending.length && at!=null && (timestampMs(pending[index].timestamp)??0)<=at)merged.push(pending[index++]);
    merged.push(row);
  }
  merged.push(...pending.slice(index));
  return merged;
}
