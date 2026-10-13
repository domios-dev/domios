import type {JsonValue} from "@codeterm/plugin-sdk";

export function canonicalJson(value:unknown, indentation=0): string {
  const normalized=JSON.parse(JSON.stringify(value ?? null)) as JsonValue;
  const gap=" ".repeat(Math.max(0,Math.min(10,Math.trunc(indentation))));
  const render=(node:JsonValue,depth:number):string=>{
    if(node === null || typeof node !== "object") return JSON.stringify(node);
    const parts=Array.isArray(node) ? node.map(entry=>render(entry,depth+1))
      : Object.keys(node).sort().map(key=>`${JSON.stringify(key)}:${gap ? " " : ""}${render(node[key],depth+1)}`);
    const [open,close]=Array.isArray(node) ? ["[","]"] : ["{","}"];
    if(!parts.length)return open+close;
    return gap ? `${open}\n${gap.repeat(depth+1)}${parts.join(`,\n${gap.repeat(depth+1)}`)}\n${gap.repeat(depth)}${close}`
      : open+parts.join(",")+close;
  };
  return render(normalized,0);
}
