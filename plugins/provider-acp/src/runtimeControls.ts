import type {JsonValue, RuntimeSettings, RuntimeAuthentication} from "@codeterm/plugin-sdk";
import {authenticationState, type AuthMethod} from "./auth";
const object=(value:unknown):Record<string,JsonValue>=>value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string,JsonValue> : {};
const label=(value:unknown):value is string=>typeof value === "string" && value.length > 0 && value.length <= 256 && !/[\u0000-\u001f]/.test(value);
function list(value:unknown, limit:number):Record<string,JsonValue>[] {
  return Array.isArray(value) && value.length <= limit ? value.map(object) : [];
}
export function commands(value:unknown):NonNullable<RuntimeSettings["commands"]> {
  const seen=new Set<string>();
  return list(value,128).flatMap(command=>{
    if (!label(command.name) || /[\s/]/.test(command.name) || seen.has(command.name)) return [];
    seen.add(command.name);
    const hint=object(command.input).hint;
    return [{name:command.name,description:typeof command.description === "string" ? command.description.slice(0,4096) : "",
      ...(typeof hint === "string" ? {argumentHint:hint.slice(0,4096)} : {})}];
  });
}
export function configOptions(value:unknown):NonNullable<RuntimeSettings["configOptions"]> {
  const seen=new Set<string>();
  return list(value,64).flatMap(config=>{
    if (!label(config.id) || !label(config.name) || seen.has(config.id)) return [];
    seen.add(config.id);
    const values=list(config.options,128).flatMap(entry=>Array.isArray(entry.options) ? list(entry.options,128) : [entry]);
    const ids=new Set<string>();
    const options=values.slice(0,128).flatMap(entry=>{
      if (!label(entry.value) || !label(entry.name) || ids.has(entry.value)) return [];
      ids.add(entry.value);return [{value:entry.value,name:entry.name}];
    });
    if (config.type !== "select" || !label(config.currentValue)) return [{id:config.id,name:config.name,currentValue:String(config.currentValue).slice(0,256) || "Unknown",options:[]}];
    return [{id:config.id,name:config.name,currentValue:config.currentValue,options}];
  });
}
export function sessionSettings(value:unknown):RuntimeSettings {
  const response=object(value), modes=object(response.modes);
  const seen=new Set<string>();
  return {...(Array.isArray(response.configOptions) ? {configOptions:configOptions(response.configOptions)} : {}),
    ...(Array.isArray(modes.availableModes) ? {modes:list(modes.availableModes,64).flatMap(mode=>{
      if (!label(mode.id) || !label(mode.name) || seen.has(mode.id)) return [];
      seen.add(mode.id);return [{id:mode.id,name:mode.name}];
    })} : {}),
    ...(label(modes.currentModeId) && seen.has(modes.currentModeId) ? {mode:modes.currentModeId} : {})};
}
export function authentication(methods:AuthMethod[], authenticated?:boolean, required?:boolean):RuntimeAuthentication {
  return {state:authenticationState(methods,{authenticated,required}),methods:methods.map(method=>({id:method.id,name:method.name,available:method.type === "agent"}))};
}
