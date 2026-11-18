import type {HostIoSteps, HostOwnedCapabilities} from "@codeterm/plugin-sdk";

export type CliGeneration = 1 | 2;

export function generation(version: unknown): CliGeneration | undefined {
  if (typeof version !== "string") return;
  if (/^1\.\d+\.\d+(?:[-+].*)?$/.test(version)) return 1;
  if (/^2\.3\.\d+(?:[-+].*)?$/.test(version)) return 2;
}

export function* cliGeneration(): HostIoSteps<CliGeneration> {
  let observation: unknown;
  try { observation = yield {op:"providerVersion"}; } catch {}
  const version = observation && typeof observation === "object" ? (observation as {version?:unknown}).version : undefined;
  const known = generation(version);
  if (known) return known;
  if (version) throw new Error(`Unsupported OpenCode CLI version ${String(version)}; supported launch contracts are 1.x and 2.3.x`);
  const result = yield* host.io.exec({bin:"opencode",args:["--help"],timeoutMs:10000});
  const help = result.stdout ?? "";
  if (result.code !== 0 || result.error || result.truncated || help.length > 65536) throw new Error("OpenCode CLI capabilities could not be read");
  if (/--standalone\b/.test(help) && /--server\b/.test(help) && !/--hostname\b/.test(help) && !/--model\b/.test(help)) return 2;
  if (/--hostname\b/.test(help) && /--port\b/.test(help) && /--model\b/.test(help)) return 1;
  throw new Error("Unsupported OpenCode CLI launch capabilities");
}

export function v2Arguments(args: string[]): {args:string[];config:Record<string,unknown>} {
  const retained:string[]=[];
  let model:string|undefined, variant:string|undefined, agent:string|undefined;
  for (let index=0; index<args.length; index++) {
    const arg=String(args[index]);
    const take=(names:string[]) => names.includes(arg) ? String(args[++index] ?? "") : names.some(name=>arg.startsWith(name+"=")) ? arg.slice(arg.indexOf("=")+1) : undefined;
    const selectedModel=take(["-m","--model"]);
    if (selectedModel !== undefined) {model=selectedModel;continue;}
    const selectedVariant=take(["--variant"]);
    if (selectedVariant !== undefined) {variant=selectedVariant;continue;}
    const selectedAgent=take(["--agent"]);
    if (selectedAgent !== undefined) {agent=selectedAgent;continue;}
    if (["--hostname","--port","--server","--standalone"].some(name=>arg===name || arg.startsWith(name+"="))) throw new Error("OpenCode server ownership is controlled by the host");
    retained.push(arg);
  }
  if (model && !/^[^/#\s]+\/[^#\s]+(?:#[^#\s]+)?$/.test(model)) throw new Error("OpenCode 2.x requires a qualified provider/model selection");
  if (variant && !/^[a-zA-Z0-9_.-]+$/.test(variant)) throw new Error("Invalid OpenCode model variant");
  if (variant && !model) throw new Error("OpenCode 2.x requires an explicit model for a variant override");
  const selected=model ? model.split("#")[0]+(variant && variant!=="default" ? `#${variant}` : variant===undefined && model.includes("#") ? `#${model.split("#")[1]}` : "") : undefined;
  return {args:retained,config:{...(selected?{model:selected}:{}),...(agent?{default_agent:agent}:{})}};
}

export function v2ConfigCommand(command:string, launchConfig:Record<string,unknown>, instructions:string[]|undefined, resumeSession:string|undefined, hostOwned?:HostOwnedCapabilities):string {
  const permissions=[{action:"*",resource:"*",effect:"ask"}];
  const pad=host.envGet("CODETERM_PAD_DIR")?.replace(/\\/g,"/").replace(/\/+$/,"");
  if (pad) permissions.push({action:"*",resource:pad+"/*",effect:"allow"});
  permissions.push(...Object.entries({question:hostOwned?.questions,todoread:hostOwned?.todos,todowrite:hostOwned?.todos}).filter(([,enabled])=>enabled).map(([action])=>({action,resource:"*",effect:"deny"})));
  const config={...launchConfig,permissions,...(instructions?{instructions}:{})};
  const platform=host.platform(), quote=(value:string)=>host.shell.quoteFor(value,platform);
  if (platform==="windows") return `$ct_previous_config = $env:OPENCODE_CONFIG_CONTENT; $ct_previous_resume = $env:CODETERM_RESUME_SESSION_ID; $env:OPENCODE_CONFIG_CONTENT = ${quote(JSON.stringify(config))}; $env:CODETERM_RESUME_SESSION_ID = ${quote(resumeSession??"")}; try { ${command} } finally { $env:OPENCODE_CONFIG_CONTENT = $ct_previous_config; $env:CODETERM_RESUME_SESSION_ID = $ct_previous_resume }`;
  return `( export OPENCODE_CONFIG_CONTENT=${quote(JSON.stringify(config))}; export CODETERM_RESUME_SESSION_ID=${quote(resumeSession??"")}; ${command} )`;
}
