import type {AgentEvent, Operation, RuntimeContext, RuntimeFrame, RuntimePlan} from "@codeterm/plugin-sdk";
import {fail, json, list, object, own, text} from "./runtimeProtocol";

function questions(params:Record<string,unknown>):Record<string,unknown>[] {
  const entries = list(params.questions).map(object);
  if (!entries.length || entries.length > 16 || entries.some(question => !text(question.question) || list(question.options).length > 32)) fail("invalid_schema","Unsupported OpenCode question shape");
  return entries;
}
export function requestPrompt(ctx:RuntimeContext, frame:Extract<RuntimeFrame,{kind:"request"}>):AgentEvent|undefined {
  const event = object(frame.params), params = object(event.properties);
  if (params.sessionID !== ctx.threadId || params.id !== frame.requestId) return undefined;
  if (frame.method === "permission.asked") {
    const permission = text(params.permission);
    if (!permission) fail("invalid_schema","Permission request has no declared action");
    const options = [{label:"Allow once",value:"once"},{label:"Always allow",value:"always"},{label:"Reject",value:"reject"}];
    return {kind:"prompt",threadId:ctx.threadId!,requestId:frame.requestId,
      prompt:{question:`${permission}: ${list(params.patterns).filter(value => typeof value === "string").join(", ")}`,options},
      permission:{action:["bash","shell"].includes(permission) ? "command" : ["edit","write"].includes(permission) ? "write" : ["read","glob","grep"].includes(permission) ? "read" : "other",
        options:[{value:"once",decision:"allow_once"},{value:"always",decision:"allow_always"},{value:"reject",decision:"reject_once"}]}};
  }
  if (frame.method !== "question.asked") return undefined;
  const entries = questions(params);
  return {kind:"prompt",threadId:ctx.threadId!,requestId:frame.requestId,prompt:{
    question:entries.map(question => String(question.question)).join("\n\n"),
    options:entries.flatMap((question,index) => list(question.options).map((raw,optionIndex) => {
      const option = object(raw), label = text(option.label);
      if (!label) fail("invalid_schema","Question option has no label");
      return {label:entries.length > 1 ? `${String(question.header ?? index+1)}: ${label}` : label,value:`${index}:${optionIndex}`};
    })),multi_select:entries.length > 1 || entries[0].multiple === true,submit_available:true}};
}
export function answerPlan(ctx:RuntimeContext, op:Extract<Operation,{kind:"answer"}>):RuntimePlan {
  const pending = ctx.pendingRequest;
  if (!pending || pending.requestId !== op.requestId) fail("binding_mismatch","Answer has no exact pending OpenCode request");
  const params = object(object(pending.params).properties);
  own(ctx,text(params.sessionID));
  if (params.id !== op.requestId) fail("binding_mismatch","Answer request identity differs");
  if (pending.method === "permission.asked") {
    const value = op.answer.kind === "cancel" ? "reject" : op.answer.kind === "choice" && op.answer.values.length === 1 ? op.answer.values[0] : undefined;
    if (!value || !["once","always","reject"].includes(value)) fail("invalid_schema","Invalid permission answer");
    return {action:{kind:"reply",requestId:op.requestId,result:{kind:"success",value:{reply:value}}},state:null};
  }
  if (pending.method !== "question.asked") fail("unsupported","Unsupported OpenCode server request");
  if (op.answer.kind === "cancel") return {action:{kind:"reply",requestId:op.requestId,result:{kind:"error",error:{code:-32800,message:"Question canceled"}}},state:null};
  const entries = questions(params);
  const answer = op.answer;
  const answers = entries.map((question,index) => {
    if (answer.kind === "text") {
      if (entries.length !== 1 || question.custom === false || !answer.text.trim()) fail("invalid_schema","This question requires its declared choices");
      return [answer.text];
    }
    const options = list(question.options).map(object);
    const values = answer.values.filter(value => value.startsWith(`${index}:`)).map(value => {
      if (!/^\d+:\d+$/.test(value)) fail("invalid_schema","Invalid question option identity");
      const option = options[Number(value.split(":")[1])], label = text(option?.label);
      if (!label) fail("invalid_schema","Unknown question option");
      return label;
    });
    if (!values.length || question.multiple !== true && values.length !== 1) fail("invalid_schema","Choose the required answers for every question");
    return values;
  });
  if (answer.kind === "choice" && answer.values.some(value => !entries[Number(value.split(":")[0])])) fail("invalid_schema","Answer contains a foreign question");
  return {action:{kind:"reply",requestId:op.requestId,result:{kind:"success",value:json({answers})}},state:null};
}
