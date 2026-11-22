import type { JsonValue, PendingPrompt, PromptAnswer, RpcId, RuntimeContext, RuntimeFailure, RuntimePermission, RuntimePermissionAction, RuntimePermissionDecision } from "@codeterm/plugin-sdk";
import { diff } from "./runtimeReduceTools";
import { methods } from "./runtimeProtocol";

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
  return path.replace(/^(?:\\\\\?\\|\/\/\?\/)(?=[a-z]:[\\/])/i,"").replace(/[\x00-\x1f\x7f]/g,"\uFFFD");
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
function permissionDiffs(tool:Record<string,JsonValue>):JsonValue[] {
  if (!Array.isArray(tool.content)) return [];
  const diffs:JsonValue[]=[];
  for (const entry of tool.content.slice(0,4)) {
    const content=object(entry);
    if (content.type !== "diff") continue;
    const mapped=diff(content);
    if (mapped && !mapped.omitted) diffs.push(mapped.value);
  }
  return diffs;
}

interface ElicitationField {
  name: string;
  type: "string" | "number" | "integer" | "boolean";
  title?: string;
  description?: string;
  required?: boolean;
  enum?: string[];
  enumNames?: string[];
  format?: string;
  minLength?: number;
  maxLength?: number;
  minimum?: number;
  maximum?: number;
  default?: JsonValue;
}

function parseSafeHttpUrl(raw:string):{url:string;host:string}|undefined {
  const trimmed=raw.trim();
  if (!trimmed || trimmed.length > 2048 || /[\x00-\x1f\x7f\s]/.test(trimmed)) return undefined;
  const match=/^(https?):\/\/([^/?#]+)([/?#].*)?$/i.exec(trimmed);
  if (!match) return undefined;
  const scheme=match[1].toLowerCase();
  const authority=match[2];
  if (authority.includes("@")) return undefined;
  const hostMatch=/^(\[[0-9a-fA-F:]+\]|[A-Za-z0-9.-]+)(?::(\d{1,5}))?$/.exec(authority);
  if (!hostMatch) return undefined;
  const host=hostMatch[1].toLowerCase();
  if (hostMatch[2]) {
    const port=Number(hostMatch[2]);
    if (!Number.isSafeInteger(port) || port < 1 || port > 65535) return undefined;
  }
  const loopback=host === "localhost" || host === "127.0.0.1" || host === "[::1]";
  if (scheme === "http" && !loopback) return undefined;
  return {url:trimmed,host:authority};
}

function decodeElicitationFields(schemaRaw:JsonValue|undefined):{fields:ElicitationField[];required:string[]}|undefined {
  const schema=object(schemaRaw);
  const props=object(schema.properties);
  const propKeys=Object.keys(props);
  if (propKeys.length > 32) return undefined;
  const requiredSet=new Set<string>(
    Array.isArray(schema.required) ? schema.required.filter((item):item is string=>typeof item === "string" && item.length > 0) : [],
  );
  const fields:ElicitationField[]=[];
  for (const name of propKeys) {
    if (!name || name.length > 128) continue;
    const spec=object(props[name]);
    const type=spec.type;
    if (type !== "string" && type !== "number" && type !== "integer" && type !== "boolean") return undefined;
    const field:ElicitationField={name,type,...(requiredSet.has(name) ? {required:true} : {})};
    if (typeof spec.title === "string" && spec.title.trim()) field.title=readable(spec.title,128).trim();
    if (typeof spec.description === "string" && spec.description.trim()) field.description=readable(spec.description,256).trim();
    if (type === "string") {
      if (Array.isArray(spec.enum) && spec.enum.length > 0 && spec.enum.length <= 64 && spec.enum.every(item=>typeof item === "string")) {
        field.enum=spec.enum as string[];
        if (Array.isArray(spec.enumNames) && spec.enumNames.length === spec.enum.length && spec.enumNames.every(item=>typeof item === "string"))
          field.enumNames=(spec.enumNames as string[]).map(item=>readable(item,128));
      }
      if (typeof spec.format === "string" && ["email","uri","date","date-time","password"].includes(spec.format)) field.format=spec.format;
      if (typeof spec.minLength === "number" && Number.isSafeInteger(spec.minLength) && spec.minLength >= 0) field.minLength=spec.minLength;
      if (typeof spec.maxLength === "number" && Number.isSafeInteger(spec.maxLength) && spec.maxLength >= 0) field.maxLength=spec.maxLength;
      if (typeof spec.default === "string") field.default=readable(spec.default,512);
    } else if (type === "number" || type === "integer") {
      if (typeof spec.minimum === "number" && Number.isFinite(spec.minimum)) field.minimum=spec.minimum;
      if (typeof spec.maximum === "number" && Number.isFinite(spec.maximum)) field.maximum=spec.maximum;
      if (typeof spec.default === "number" && Number.isFinite(spec.default)) field.default=spec.default;
    } else if (typeof spec.default === "boolean") {
      field.default=spec.default;
    }
    fields.push(field);
  }
  return {fields,required:fields.filter(field=>field.required).map(field=>field.name)};
}

function elicitationPrompt(data:Record<string,JsonValue>):PromptRequest|undefined {
  const mode=data.mode === "url" ? "url" : data.mode === undefined || data.mode === "form" ? "form" : undefined;
  if (!mode) return undefined;
  const message=readable(data.message,1024).trim();
  if (mode === "url") {
    const parsed=typeof data.url === "string" ? parseSafeHttpUrl(data.url) : undefined;
    if (!parsed) return undefined;
    const question=message || `Open ${parsed.host} to sign in`;
    const elicitation:JsonValue={mode:"url",message:question,url:parsed.url,host:parsed.host};
    return {
      prompt:{
        question,
        options:[{label:"Open sign-in",value:"accept"},{label:"Decline",value:"decline"}],
        elicitation,
      } as PendingPrompt,
      elicitation,
    };
  }
  const decoded=decodeElicitationFields(data.requestedSchema);
  if (!decoded) return undefined;
  const question=message || "Input requested";
  const elicitation:JsonValue={mode:"form",message:question,fields:decoded.fields as unknown as JsonValue,required:decoded.required};
  return {
    prompt:{
      question,
      options:[],
      elicitation,
    } as PendingPrompt,
    elicitation,
  };
}

function validateElicitationContent(fields:readonly ElicitationField[], raw:Record<string,JsonValue>):Record<string,JsonValue> {
  const output:Record<string,JsonValue>={};
  for (const field of fields) {
    const has=Object.prototype.hasOwnProperty.call(raw,field.name) && raw[field.name] !== undefined && raw[field.name] !== null;
    if (!has) {
      if (field.required) fail(`Missing required field: ${field.title ?? field.name}`);
      continue;
    }
    const val=raw[field.name];
    if (field.type === "string") {
      if (typeof val !== "string") fail(`Field ${field.name} must be text`);
      if (field.required && !val.trim()) fail(`Field ${field.title ?? field.name} cannot be empty`);
      if (field.minLength !== undefined && val.length < field.minLength) fail(`Field ${field.name} is shorter than ${field.minLength}`);
      if (field.maxLength !== undefined && val.length > field.maxLength) fail(`Field ${field.name} exceeds ${field.maxLength} characters`);
      if (field.enum && !field.enum.includes(val)) fail(`Field ${field.name} must match an allowed option`);
      output[field.name]=val;
    } else if (field.type === "number" || field.type === "integer") {
      if (typeof val !== "number" || !Number.isFinite(val)) fail(`Field ${field.name} must be a number`);
      if (field.type === "integer" && !Number.isSafeInteger(val)) fail(`Field ${field.name} must be a whole number`);
      if (field.minimum !== undefined && val < field.minimum) fail(`Field ${field.name} must be at least ${field.minimum}`);
      if (field.maximum !== undefined && val > field.maximum) fail(`Field ${field.name} must be at most ${field.maximum}`);
      output[field.name]=val;
    } else {
      if (typeof val !== "boolean") fail(`Field ${field.name} must be true or false`);
      output[field.name]=val;
    }
  }
  return output;
}

interface PromptRequest { prompt:PendingPrompt; permission?:RuntimePermission; fileDiffs?:JsonValue[]; elicitation?:JsonValue }
export function runtimePrompt(ctx:RuntimeContext, method:string, params:JsonValue):PromptRequest|undefined {
  const data=object(params);
  if (!ctx.threadId || data.sessionId !== ctx.threadId) return undefined;
  if (method === methods.elicitation) return elicitationPrompt(data);
  if (method !== methods.requestPermission) return undefined;
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
  const fileDiffs=permissionDiffs(tool);
  const prompt:PendingPrompt={
    question,
    options:options.map(option=>({label:readable(option.name,128).trim() || kindLabels[option.kind as RuntimePermissionDecision],value:String(option.optionId)})),
    ...(fileDiffs.length ? {file_diffs:fileDiffs} : {}),
  } as PendingPrompt;
  return {
    prompt,
    permission:{action:actions[String(tool.kind)] ?? "other",options:options.map(option=>({value:String(option.optionId),decision:option.kind as RuntimePermissionDecision}))},
    ...(fileDiffs.length ? {fileDiffs} : {}),
  };
}
export function runtimeAnswer(ctx:RuntimeContext, method:string, params:JsonValue, _id:RpcId, answer:PromptAnswer):JsonValue {
  const request=runtimePrompt(ctx,method,params);
  if (!request) fail(method === methods.elicitation ? "Elicitation request does not belong to this session" : "Permission request does not belong to this session");
  if (method === methods.elicitation) {
    if (answer.kind === "cancel") return {action:"cancel"};
    const data=object(params);
    if (data.mode === "url") {
      if (answer.kind !== "choice" || answer.values.length !== 1 || !["accept","decline"].includes(answer.values[0]))
        fail("URL elicitation requires accept or decline");
      return {action:answer.values[0]};
    }
    if (answer.kind !== "text") fail("Form elicitation requires a structured response");
    let parsed:Record<string,JsonValue>;
    try { parsed=object(JSON.parse(answer.text)); } catch { fail("Form elicitation response must be valid JSON"); }
    const action=parsed.action;
    if (action === "decline" || action === "cancel") return {action};
    if (action !== "accept") fail("Form elicitation action must be accept, decline, or cancel");
    const decoded=decodeElicitationFields(data.requestedSchema);
    if (!decoded) fail("Elicitation schema is invalid");
    return {action:"accept",content:validateElicitationContent(decoded.fields,object(parsed.content))};
  }
  const prompt=request.prompt;
  if (answer.kind === "cancel") return {outcome:{outcome:"cancelled"}};
  if (answer.kind !== "choice" || answer.values.length !== 1 || !prompt.options.some(option=>option.value === answer.values[0]))
    fail("Permission requires one declared optionId");
  return {outcome:{outcome:"selected",optionId:answer.values[0]}};
}
