import type {RuntimeQuestion, RuntimeQuestionAnswer, TurnInput} from "@codeterm/plugin-sdk";
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
