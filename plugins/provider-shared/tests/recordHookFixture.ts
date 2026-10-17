import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import type { HostFs, JsonValue, ParsedSessionRecord, PluginModule, SessionRecordsMeta } from "@codeterm/plugin-sdk";

export function recordInput(text: string, fromOffset = 0, sourceKey?: string): {records: ParsedSessionRecord[]; meta: SessionRecordsMeta} {
  const records: ParsedSessionRecord[] = [];
  let offset = fromOffset, invalid = 0;
  const lines = text.split("\n");
  for (let row = 0; row < lines.length; row++) {
    const line = lines[row];
    if (line.trim()) {
      try { records.push({offset,row,value:JSON.parse(line) as JsonValue}); } catch { invalid++; }
    }
    offset += Buffer.byteLength(line) + (row + 1 < lines.length ? 1 : 0);
  }
  return {records,meta:{fromOffset,toOffset:offset,newOffset:offset,truncatedTail:false,sourceKey,
    counts:{total:lines.length-(lines.at(-1)==="" ? 1 : 0),matched:records.length,invalid,oversized:0}}};
}

export function loadRecordPlugin(directory: string, fsOverrides: Partial<HostFs> = {}): PluginModule {
  const context = {module:{exports:{} as {default:PluginModule}},exports:{},console,
    host:{platform:()=>"linux",unixNowMs:()=>0,manifestJson:()=>"null",envGet:()=>null,homeDir:()=>"/tmp",
      expandHome:(value:string)=>value,path:{equal:()=>true},shell:{quoteFor:String,quote:String},
      fs:{fileExists:()=>false,readFile:()=>null,readFileHead:()=>null,readFileTail:()=>null,readJson:()=>null,...fsOverrides}}};
  vm.runInNewContext(fs.readFileSync(path.join(directory,"plugin.js"),"utf8"),context);
  return context.module.exports.default;
}

export function plain<T>(value: T): T { return JSON.parse(JSON.stringify(value)) as T; }
