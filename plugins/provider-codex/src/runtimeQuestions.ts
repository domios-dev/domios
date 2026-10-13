import type {RuntimeQuestion, RuntimeQuestionAnswer, RuntimeHistoryQuestion, TurnInput} from "@codeterm/plugin-sdk";
import {list,object,text} from "./runtimeProtocol";

export function nativeOption(value:unknown): {value:string; label:string} | undefined {
  if (typeof value === "string") return {value,label:value};
  const option=object(value);const label=text(option.label) ?? text(option.title);
  const id=text(option.value) ?? text(option.id) ?? label;
  return label && id ? {value:id,label} : undefined;
}
export function runtimeQuestions(value:unknown): RuntimeQuestion[] {
  return list(value).map(object).map((question,index)=>({
    id:text(question.id) ?? String(index),text:text(question.title) ?? text(question.question) ?? "Choose an answer",
    options:list(question.options).flatMap(option=>{const parsed=nativeOption(option);return parsed ? [parsed] : [];}),
    ...(typeof question.multiSelect === "boolean" ? {multiSelect:question.multiSelect} : {}),
    ...(typeof question.allowFreeText === "boolean" ? {allowFreeText:question.allowFreeText} : {}),
  }));
}
export function questionAnswerInput(itemId:string, answers:readonly RuntimeQuestionAnswer[]): TurnInput {
  return {text:JSON.stringify({itemId,answers:answers.map(answer=>({questionId:answer.questionId,values:[...answer.values],...(answer.text !== undefined ? {text:answer.text} : {})}))}),attachments:[]};
}

function answersMatch(questions:readonly RuntimeQuestion[], raw:unknown): boolean {
  const answers=list(raw).map(object);
  if (answers.length !== questions.length || new Set(answers.map(answer=>answer.questionId)).size !== answers.length) return false;
  return questions.every(question=>{
    const answer=answers.find(answer=>answer.questionId === question.id);
    if (!answer || !Array.isArray(answer.values)) return false;
    const values=answer.values;
    if (new Set(values).size !== values.length || values.some(value=>typeof value !== "string" || !question.options.some(option=>option.value === value))) return false;
    if (!question.multiSelect && values.length > 1) return false;
    const freeText=text(answer.text);
    if (answer.text !== undefined && (!freeText || question.allowFreeText === false)) return false;
    return values.length > 0 || !!freeText;
  });
}
export function reconcileHistoryQuestions(threadId:string, raw:unknown, previous:readonly RuntimeHistoryQuestion[]): RuntimeHistoryQuestion[] {
  const item=object(raw);const itemId=text(item.id);
  if (item.type === "agentMessage" && item.delivery === "async" && itemId && list(item.questions).length) {
    const old=previous.find(question=>question.itemId === itemId);
    const question:RuntimeHistoryQuestion={threadId,itemId,questions:runtimeQuestions(item.questions),response:"message",...(old?.resolved ? {resolved:true} : {})};
    return [...previous.filter(existing=>existing.itemId !== itemId),question];
  }
  if (item.type !== "userMessage") return [...previous];
  const content=list(item.content).map(object).filter(part=>part.type === "text").map(part=>part.text).join("\n");
  let answer:Record<string,unknown>;
  try {answer=object(JSON.parse(content));} catch {return [...previous];}
  return previous.map(question=>answer.itemId === question.itemId && answersMatch(question.questions,answer.answers) ? {...question,resolved:true} : question);
}
