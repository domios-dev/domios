import type { OperationResult, ChatMsg, RuntimeHistoryQuestion } from "@codeterm/plugin-sdk";
import { reconcileHistoryQuestions } from "../src/runtimeQuestions";

type Page = Extract<OperationResult,{kind:"historyPage"}>;
export function collectHistory(pages:readonly OperationResult[]): Page {
  const items:ChatMsg[]=[];const openItems:Page["openItems"][number][]=[];
  let questions:RuntimeHistoryQuestion[]=[];let last:Page | undefined;
  for (const page of pages) {
    if(page.kind !== "historyPage")throw new Error("Expected history page");
    last=page;
    for(const question of page.questions ?? []) {
      const old=questions.find(q=>q.itemId === question.itemId);
      questions=[...questions.filter(q=>q.itemId !== question.itemId),{...question,...(old?.resolved ? {resolved:true} : {})}];
    }
    for(const item of page.items) {
      if(item.type === "user") questions=reconcileHistoryQuestions(page.threadId,{type:"userMessage",content:[{type:"text",text:item.content}]},questions);
    }
    items.push(...page.items);openItems.push(...page.openItems);
  }
  if(!last)throw new Error("Missing history page");
  return {...last,items,openItems,...(questions.length ? {questions} : {})};
}
