import { canonicalJson } from "./canonicalJson";
import type { ChatMsg } from "@codeterm/plugin-sdk";
import { object, text, list } from "./runtimeProtocol";
import { runtimeFileDiffs } from "./runtimeDiffs";
import { runtimeQuestions } from "./runtimeQuestions";
import { diffEdits } from "./fileDiffs";

export function json(value: unknown): import("@codeterm/plugin-sdk").JsonValue {
  return JSON.parse(JSON.stringify(value));
}

function toolOutput(value:unknown): string {
  const result=json(value);
  const parts=Array.isArray(result) ? result : list(object(result).content);
  const mapped=parts.map(part=>{
    const item=object(part);
    if(item.type === "image") return typeof item.data === "string" && item.data.startsWith("[image omitted:")
      ? {type:"text",text:item.data} : {type:"image_url",image_url:{url:`data:${item.mimeType};base64,${item.data}`}};
    if(item.type === "inputImage") return {type:"image_url",image_url:{url:item.imageUrl}};
    return part;
  });
  return display(Array.isArray(result) ? mapped : parts.length ? {...object(result),content:mapped} : result);
}

function display(value: unknown): string {
  return typeof value === "string" ? value : canonicalJson(value, 2);
}

export function runtimeItem(raw: unknown, seq: number, previous?: ChatMsg): ChatMsg | undefined {
  const item = object(raw);
  const id = text(item.id);
  if (!id) return undefined;
  const row: ChatMsg = {id, seq: previous?.seq ?? seq, type: "tool_call", content: ""};
  if (previous?.timestamp) row.timestamp = previous.timestamp;
  switch (item.type) {
    case "userMessage": {
      row.type = "user";
      row.content = list(item.content).map(object).filter(p => p.type === "text").map(p => p.text).join("\n");
      const attachments = list(item.content).map(object).flatMap((p, index) => {
        if (p.type !== "image" && p.type !== "localImage") return [];
        const url = text(p.url);
        const data = url?.match(/^data:([^;]+);base64,(.*)$/);
        return [{id:`${id}-image-${index}`, mimeType:data?.[1] ?? "image/png",
          ...(data ? {data:data[2]} : url ? {url} : {}), ...(text(p.path) ? {path:String(p.path)} : {})}];
      });
      if (attachments.length) row.attachments = attachments;
      return row;
    }
    case "toolFlow": return {...row,type:"tool_flow",content:String(item.state ?? "")};
    case "sessionIntent": return {...row,type:"custom",sessionIntent:{...(text(item.title) ? {title:String(item.title)} : {}),...(text(item.objective) ? {goal:String(item.objective)} : {})}};
    case "sleep": row.toolSummary = `Wait ${item.durationMs} ms`; break;
    case "agentMessage": case "plan":
      row.type = "assistant"; row.content = typeof item.text === "string" ? item.text : previous?.content ?? "";
      if (item.delivery === "async" && list(item.questions).length) {
        row.toolInput = item;
        row.interactive = runtimeQuestions(item.questions).map(question=>({type:"question" as const,id:question.id,toolUseId:id,prompt:question.text,options:question.options.map(option=>({...option})),multiSelect:question.multiSelect ?? false}));
      }
      return row;
    case "reasoning":
      row.type = "thinking"; row.collapsed = true;
      row.content = list(item.summary).join("\n") || list(item.content).join("\n"); return row;
    case "contextCompaction":
      return {...row, type:"system", content:"context compacted", systemSubtype:"contextCompaction", timelineMarker:{kind:"context_compaction"}};
    case "error":
      return {...row, type:"system", content:String(item.message ?? ""), systemSubtype:"api_error"};
    case "hookPrompt": case "enteredReviewMode": case "exitedReviewMode":
      return {...row, type:"system", systemSubtype:String(item.type), content:item.type === "hookPrompt"
        ? list(item.fragments).map(object).map(p => p.text ?? "").join("\n") : String(item.review ?? "")};
    case "commandExecution":
      row.toolKind = "command"; row.toolSummary = String(item.command ?? "");
      row.toolDetail = row.toolSummary;
      if (typeof item.aggregatedOutput === "string") row.toolResult = item.aggregatedOutput;
      else if (previous?.toolResult !== undefined) row.toolResult = previous.toolResult;
      if (typeof item.exitCode === "number") row.toolExitCode = item.exitCode;
      if (item.status === "interrupted") row.toolInterrupted = true;
      break;
    case "fileChange":
      row.toolKind = "edit"; row.fileDiffs = runtimeFileDiffs(item.changes);
      row.toolEdits = diffEdits(row.fileDiffs as import("./fileDiffs").FileDiff[]);
      row.toolSummary = list(item.changes).map(object).map(p => p.path).join("\n");
      row.toolDetail = row.toolSummary;
      break;
    case "mcpToolCall": case "dynamicToolCall": {
      const prefix = text(item.server) ?? text(item.namespace);
      row.toolKind = "generic"; row.toolName = [prefix,text(item.tool)].filter(Boolean).join(" · ");
      row.toolInput = item.arguments ?? null;
      row.toolSummary = display(item.arguments); row.toolTitle = row.toolName;
      if (item.result != null || item.contentItems != null) row.toolResult = toolOutput(item.result ?? item.contentItems);
      if (item.error != null) {row.toolResult = display(item.error); row.toolError = true;}
      if (item.success === false || object(item.result).isError === true) row.toolError = true;
      break;
    }
    case "functionCallOutput":
      return {...row,type:"tool_result",toolId:id,toolName:String(item.name ?? ""),toolResult:display(item.output)};
    case "webSearch":
      row.toolKind = "web"; row.toolSummary = String(item.query ?? "");
      row.toolRichResult = json({kind:"web",query:item.query,action:object(item.action).type,url:object(item.action).url,pattern:object(item.action).pattern,
        resultCount:Array.isArray(item.results) ? item.results.length : undefined});
      if (item.results != null) row.toolResult = display(item.results);
      break;
    case "imageView": case "imageGeneration":
      row.toolKind = "image"; row.toolSummary = String(item.savedPath ?? item.path ?? item.revisedPrompt ?? "");
      row.toolRichResult = json({kind:"image",origin:item.type === "imageGeneration" ? "generated" : "viewed",path:item.savedPath ?? item.path,prompt:item.revisedPrompt,status:item.status});
      if (text(item.result)) row.toolResult = String(item.result).startsWith("data:") || String(item.result).startsWith("[image omitted:") || item.type === "imageView"
        ? String(item.result) : `data:image/png;base64,${item.result}`;
      break;
    case "collabAgentToolCall": case "subAgentActivity": {
      if (item.type === "collabAgentToolCall" && !list(item.receiverThreadIds).length) {
        if (item.tool === "spawnAgent") return undefined;
        row.toolKind = "generic"; row.toolName = text(item.tool) ?? "collabAgentToolCall";
        break;
      }
      const agent = String(item.agentThreadId ?? list(item.receiverThreadIds)[0] ?? id);
      const status = String(item.kind ?? object(object(item.agentsStates)[agent]).status ?? "");
      row.toolKind = "agent"; row.toolName = "agent_activity"; row.toolSummary = agent;
      row.toolInput = json({agent_id:agent,agent_path:item.agentPath,agent_state:({pendingInit:"started",errored:"failed",shutdown:"closed"} as Record<string,string>)[status] ?? status,
        agent_call_id:id,agent_description:item.prompt,agent_model:item.model,
        agent_reasoning_effort:item.reasoningEffort,agent_ordinal:item.ordinal,
        agent_result:object(object(item.agentsStates)[agent]).message});
      break;
    }
    case "planUpdate":
      row.toolName = "update_plan"; row.toolSummary = String(item.explanation ?? ""); row.toolResult = display(item.plan); break;
    default: return undefined;
  }
  row.toolId = id; row.toolName ??= String(item.type); row.toolInput ??= item;
  if (row.toolSummary !== undefined) row.toolArgs = row.toolSummary;
  row.collapsed = true;
  if (typeof item.durationMs === "number") row.toolDurationMs = item.durationMs;
  if (row.toolError == null) {
    if (typeof item.exitCode === "number") row.toolError = item.exitCode !== 0;
    else if (["failed","declined","error"].includes(String(item.status))) row.toolError = true;
    else if (item.status === "completed") row.toolError = false;
  }
  return row;
}

export function runtimeItems(raw:unknown, seq:number): ChatMsg[] {
  const item=object(raw);
  const receivers=list(item.receiverThreadIds);
  if(item.type === "collabAgentToolCall" && receivers.length) return receivers.flatMap((receiver,index)=>{
    const row=runtimeItem({...item,id:`${item.id}:${receiver}`,receiverThreadIds:[receiver],...(item.tool === "spawnAgent" ? {ordinal:index+1} : {})},seq+index);
    return row ? [row] : [];
  });
  const row=runtimeItem(raw,seq);
  return row ? [row] : [];
}
