import type { JsonValue, PendingPrompt, PromptAnswer, RpcId, RuntimeContext, RuntimeFailure, RuntimePermission, RuntimePermissionAction, RuntimePermissionDecision } from "@codeterm/plugin-sdk";

const kinds = new Set<string>(["allow_once", "allow_always", "reject_once", "reject_always"] satisfies RuntimePermissionDecision[]);
const kindLabels:Record<RuntimePermissionDecision,string> = {allow_once:"Allow once",allow_always:"Always allow",reject_once:"Reject",reject_always:"Always reject"};
const actions:Record<string,RuntimePermissionAction> = {read:"read",search:"read",edit:"write",delete:"write",move:"write",execute:"command"};
const object = (value:unknown):Record<string,JsonValue> =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string,JsonValue> : {};
function fail(message:string):never { throw {code:"provider_refused",message} satisfies RuntimeFailure; }
function readable(value:JsonValue|undefined, limit=384):string {
  if (typeof value !== "string") return "";
  const text=value.slice(0,limit+1).replace(/\r\n?/g,"\n").replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g,"");
  if (value.length <= limit) return text;
  let end=Math.min(text.length,limit-1);
  if (/[\uD800-\uDBFF]/.test(text[end-1])) end--;
  return `${text.slice(0,end)}…`;
}
function lineCount(value:JsonValue|undefined):number {
  if (typeof value !== "string" || !value.length) return 0;
  let count=value.endsWith("\n") ? 0 : 1;
  for (let i=0;i<value.length;i++) if (value[i] === "\n") count++;
  return count;
}
function preview(value:JsonValue|undefined, prefix=""):string[] {
  const text=readable(value);
  if (!text) return [];
  const lines=text.split("\n",4);
  if (lines[lines.length-1] === "") lines.pop();
  return lines.slice(0,3).map(line=>`${prefix}${line}`).concat(lines.length > 3 ? ["…"] : []);
}
function displayPath(path:string):string {
  if (path.startsWith("\\\\?\\UNC\\")) return "\\\\"+path.slice(8);
  return path.replace(/^(?:\\\\\?\\|\/\/\?\/)(?=[a-z]:[\\/])/i,"").replace(/[\x00-\x1f\x7f]/g,"�");
}
function writeHeader(tool:Record<string,JsonValue>):string {
  if (tool.kind !== "edit" || !Array.isArray(tool.content)) return "";
  const diffs=tool.content.map(object).filter(entry=>entry.type === "diff");
  const path=diffs[0]?.path;
  if (diffs.length !== 1 || typeof path !== "string" || !path.length || path.length > 4096) return "";
  return `Writes ${displayPath(path)} (+${lineCount(diffs[0].newText)}/-${lineCount(diffs[0].oldText)} lines)\n`;
}
function permissionQuestion(tool:Record<string,JsonValue>):string {
  const title=readable(tool.title,256).trim() || "Tool permission requested";
  const kind=readable(tool.kind,64).trim();
  const lines=[kind && kind !== "other" ? `${title}\nKind: ${kind}` : title];
  const paths=new Set<string>();
  const addPath=(value:JsonValue|undefined):void=>{
    const path=readable(value,512);
    if (path && !paths.has(path) && paths.size < 4) { paths.add(path);lines.push(`Path: ${path}`); }
  };
  if (Array.isArray(tool.locations)) for (const location of tool.locations.slice(0,4)) addPath(object(location).path);
  const input=object(tool.rawInput);
  addPath(input.path);
  const command=readable(input.command || input.commandLine || input.cmd,768);
  if (command) lines.push(`Command: ${command}`);
  if (Array.isArray(tool.content)) for (const entry of tool.content.slice(0,4)) {
    const content=object(entry);
    if (content.type === "diff") {
      addPath(content.path);
      lines.push(`Diff: ${readable(content.path,512) || "file"} (+${lineCount(content.newText)} added, -${lineCount(content.oldText)} removed lines)`);
      lines.push(...preview(content.oldText,"- "),...preview(content.newText,"+ "));
    } else if (content.type === "content") {
      const block=object(content.content);
      if (block.type === "text") lines.push(...preview(block.text));
    }
  }
  lines.push("Allow this tool call?");
  const header=writeHeader(tool);
  const budget=Math.max(0,2048-header.length);
  return header+(budget ? readable(lines.join("\n"),budget) : "");
}
interface PermissionRequest { prompt:PendingPrompt; permission:RuntimePermission }
export function runtimePrompt(ctx:RuntimeContext, method:string, params:JsonValue):PermissionRequest|undefined {
  const data=object(params);
  if (method !== "session/request_permission" || !ctx.threadId || data.sessionId !== ctx.threadId) return undefined;
  const tool=object(data.toolCall);
  if (typeof tool.toolCallId !== "string" || !tool.toolCallId || !Array.isArray(data.options) || !data.options.length || data.options.length > 16) return undefined;
  const seen=new Set<string>();
  const options=data.options.map(object);
  if (options.some(option=>{
    if (typeof option.optionId !== "string" || !option.optionId || option.optionId.length > 4096
      || seen.has(option.optionId) || typeof option.name !== "string" || option.name.length > 4096
      || typeof option.kind !== "string" || !kinds.has(option.kind)) return true;
    seen.add(option.optionId);
    return false;
  })) return undefined;
  const question=permissionQuestion(tool);
  return {
    prompt:{question,options:options.map(option=>({label:readable(option.name,128).trim() || kindLabels[option.kind as RuntimePermissionDecision],value:String(option.optionId)}))},
    permission:{action:actions[String(tool.kind)] ?? "other",options:options.map(option=>({value:String(option.optionId),decision:option.kind as RuntimePermissionDecision}))},
  };
}
export function runtimeAnswer(ctx:RuntimeContext, method:string, params:JsonValue, _id:RpcId, answer:PromptAnswer):JsonValue {
  const prompt=runtimePrompt(ctx,method,params)?.prompt;
  if (!prompt) fail("Permission request does not belong to this session");
  if (answer.kind === "cancel") return {outcome:{outcome:"cancelled"}};
  if (answer.kind !== "choice" || answer.values.length !== 1 || !prompt.options.some(option=>option.value === answer.values[0]))
    fail("Permission requires one declared optionId");
  return {outcome:{outcome:"selected",optionId:answer.values[0]}};
}
