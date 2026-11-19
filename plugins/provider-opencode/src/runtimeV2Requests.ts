import type {AgentEvent, Operation, RuntimeContext, RuntimeFrame, RuntimePlan} from "@codeterm/plugin-sdk";
import {fail, json, list, object, own, text} from "./runtimeProtocol";

function fields(form:Record<string,unknown>):Record<string,unknown>[] {
  const entries=list(form.fields).map(object);
  if (!entries.length || entries.length > 16 || entries.some(field=>!text(field.key) || !["string","multiselect"].includes(String(field.type)) || field.hidden || list(field.when).length || list(field.options).length > 32)) fail("unsupported","Unsupported OpenCode question form");
  return entries;
}
export function v2Prompt(ctx:RuntimeContext, frame:Extract<RuntimeFrame,{kind:"request"}>):AgentEvent|undefined {
  const params=object(object(frame.params).data);
  if (frame.method === "permission.asked") {
    if (params.sessionID !== ctx.threadId || params.id !== frame.requestId) return undefined;
    const action=text(params.action);
    if (!action) fail("invalid_schema","Permission request has no action");
    // Always may settle sibling requests before their automatic replies reach OpenCode.
    return {kind:"prompt",threadId:ctx.threadId!,requestId:frame.requestId,prompt:{question:`${action}: ${list(params.resources).filter(value=>typeof value === "string").join(", ")}`,options:[{label:"Allow once",value:"once"},{label:"Always allow",value:"always"},{label:"Reject",value:"reject"}]},
      permission:{action:action === "shell" ? "command" : ["edit","write"].includes(action) ? "write" : ["read","glob","grep"].includes(action) ? "read" : "other",options:[{value:"once",decision:"allow_once"},{value:"reject",decision:"reject_once"}]}};
  }
  const form=object(params.form);
  if (frame.method !== "form.created" || form.sessionID !== ctx.threadId || form.id !== frame.requestId) return undefined;
  const entries=fields(form);
  return {kind:"prompt",threadId:ctx.threadId!,requestId:frame.requestId,prompt:{question:String(form.title),options:entries.flatMap((field,index)=>list(field.options).map((raw,optionIndex)=>{
    const option=object(raw), label=text(option.label);
    if (!label || !text(option.value)) fail("invalid_schema","Question option is missing its identity");
    return {label:entries.length > 1 ? `${String(field.title ?? field.key)}: ${label}` : label,value:`${index}:${optionIndex}`};
  })),multi_select:entries.length > 1 || entries[0].type === "multiselect",submit_available:true}};
}
export function v2Answer(ctx:RuntimeContext, op:Extract<Operation,{kind:"answer"}>):RuntimePlan {
  const pending=ctx.pendingRequest;
  if (!pending || pending.requestId !== op.requestId) fail("binding_mismatch","Answer has no exact pending OpenCode request");
  const data=object(object(pending.params).data);
  if (pending.method === "permission.asked") {
    own(ctx,text(data.sessionID));
    if (data.id !== op.requestId) fail("binding_mismatch","Permission identity differs");
    const decision=op.answer.kind === "cancel" ? "reject" : op.answer.kind === "choice" && op.answer.values.length === 1 ? op.answer.values[0] : undefined;
    if (!decision || !["once","always","reject"].includes(decision)) fail("invalid_schema","Invalid OpenCode permission decision");
    return {action:{kind:"reply",requestId:op.requestId,result:{kind:"success",value:{decision}}},state:null};
  }
  const form=object(data.form);
  own(ctx,text(form.sessionID));
  if (pending.method !== "form.created" || form.id !== op.requestId) fail("binding_mismatch","Question form identity differs");
  if (op.answer.kind === "cancel") return {action:{kind:"reply",requestId:op.requestId,result:{kind:"error",error:{code:-32800,message:"Question canceled"}}},state:null};
  const entries=fields(form), answer:Record<string,unknown>={}, selected=op.answer;
  if (selected.kind === "text") {
    if (entries.length !== 1 || entries[0].type !== "string" || entries[0].custom === false || !selected.text.trim()) fail("invalid_schema","This question requires its declared choices");
    answer[String(entries[0].key)]=selected.text;
  } else {
    const accepted=new Set<string>();
    entries.forEach((field,index)=>{
      const options=list(field.options).map(object);
      const values=selected.values.filter(value=>value.startsWith(`${index}:`)).map(value=>{
        if (!/^\d+:\d+$/.test(value)) fail("invalid_schema","Invalid question option identity");
        const option=options[Number(value.split(":")[1])], identity=text(option?.value);
        if (!identity || accepted.has(value)) fail("invalid_schema","Unknown or duplicate question choice");
        accepted.add(value);return identity;
      });
      if (!values.length && field.required !== false || field.type === "string" && values.length > 1) fail("invalid_schema","Choose the required answers for every question");
      answer[String(field.key)]=field.type === "multiselect" ? values : values[0] ?? "";
    });
    if (accepted.size !== selected.values.length) fail("invalid_schema","Answer includes a foreign question");
  }
  return {action:{kind:"reply",requestId:op.requestId,result:{kind:"success",value:json({answer})}},state:null};
}
