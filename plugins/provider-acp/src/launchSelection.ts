import type {RuntimeSettings} from "@codeterm/plugin-sdk";

export type LaunchSelection = {kind:"complete"} | {kind:"refused";message:string}
  | {kind:"set";category:string;id:string;value:string};

export function launchSelection(requested:RuntimeSettings, reported:RuntimeSettings | undefined, attempted:readonly string[]):LaunchSelection {
  for (const [category,value,title] of [["model",requested.model,"model"],["thought_level",requested.effort,"reasoning effort"]] as const) {
    if (!value) continue;
    const option=reported?.configOptions?.find(option=>option.category === category);
    const offered=option?.options.map(choice=>choice.value).join(", ") || "none";
    const choices=Array.from(offered).slice(0,2000).join("");
    if (!option || !option.options.some(choice=>choice.value === value))
      return {kind:"refused",message:`Requested ${title} '${value}' is not offered; offered values: ${choices}`};
    if (option.currentValue === value) continue;
    if (attempted.includes(category))
      return {kind:"refused",message:`Agent did not confirm requested ${title} '${value}'; offered values: ${choices}`};
    return {kind:"set",category,id:option.id,value};
  }
  return {kind:"complete"};
}
