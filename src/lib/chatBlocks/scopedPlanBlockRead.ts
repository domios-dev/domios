import type { CommittedPlanChange } from "../../components/Canvas/planProjection";

export interface ScopedBlockReadState<T> { data: T | null; error: string | null; loaded: boolean }
interface Entry<T> { state: ScopedBlockReadState<T>; users: number; controller: AbortController | null; floor: string }

export function createScopedPlanBlockRead<T>(ports: {
  read(key: string, signal: AbortSignal): Promise<T>;
  watch(changed: (change?: CommittedPlanChange) => void): { stop(): void; ready: Promise<void> };
  epoch(): unknown;
  targets?(key: string, value: T | null, change: CommittedPlanChange): boolean;
  revision?(value: T): string;
}) {
  const entries = new Map<string,Entry<T>>();
  const listeners = new Set<()=>void>();
  let epoch = ports.epoch();
  let watching: { stop():void; ready:Promise<void> } | null = null;
  const notify = () => { for (const listener of [...listeners]) listener(); };
  const checkEpoch = () => {
    const next = ports.epoch();
    if (next === epoch) return;
    epoch = next;
    for (const entry of entries.values()) { entry.controller?.abort(); entry.controller=null; entry.state={data:null,error:null,loaded:false}; entry.floor="0"; }
  };
  const entryFor = (key: string) => {
    checkEpoch();
    let entry = entries.get(key);
    if (!entry) {
      for (const [old,cached] of entries) if (entries.size >= 128 && cached.users === 0) { cached.controller?.abort(); entries.delete(old); }
      entry={state:{data:null,error:null,loaded:false},users:0,controller:null,floor:"0"}; entries.set(key,entry);
    }
    return entry;
  };
  const refresh = (key: string) => {
    const entry=entryFor(key);
    if (!entry.users) return;
    entry.controller?.abort();
    const controller=new AbortController(); entry.controller=controller;
    const source=epoch;
    void (watching?.ready ?? Promise.resolve()).then(()=>{
      if(controller.signal.aborted)throw new DOMException("View released","AbortError");
      return ports.read(key,controller.signal);
    }).then((data)=>{
      if (controller.signal.aborted || source!==ports.epoch() || entries.get(key)!==entry) return;
      const revision=ports.revision?.(data);
      if (revision && BigInt(revision)<BigInt(entry.floor)) throw new Error("Plan view predates a committed change.");
      if (revision) entry.floor=revision;
      entry.state={data,error:null,loaded:true}; notify();
    }).catch((cause: unknown)=>{
      if (controller.signal.aborted || source!==ports.epoch() || entries.get(key)!==entry) return;
      const status=cause && typeof cause==="object" && "status" in cause ? cause.status : null;
      const unavailable=typeof status==="number" && [400,401,403,404,410].includes(status);
      entry.state={data:unavailable ? null : entry.state.data,error:cause instanceof Error ? cause.message : String(cause),loaded:true}; notify();
    }).finally(()=>{if(entry.controller===controller) entry.controller=null;});
  };
  const changed=(change?: CommittedPlanChange)=>{
    checkEpoch();
    for(const [key,entry] of entries){
      if(!entry.users || change && ports.targets && !ports.targets(key,entry.state.data,change)) continue;
      if(change && ports.revision && entry.state.data && BigInt(change.revision)<=BigInt(ports.revision(entry.state.data))) continue;
      if(change) entry.floor=change.revision;
      else entry.floor="0";
      if(change?.deleted) entry.state={data:null,error:null,loaded:false};
      refresh(key);
    }
    notify();
  };
  return {
    read:(key:string)=>{
      const entry=entryFor(key);
      if(entry.users && !entry.state.loaded && !entry.controller) queueMicrotask(()=>refresh(key));
      return entry.state;
    },
    refresh,
    retain(key:string){
      const entry=entryFor(key); entry.users++;
      if(!watching) watching=ports.watch(changed);
      return ()=>{
        entry.users--;
        if(!entry.users){entry.controller?.abort();entries.delete(key);}
        if(![...entries.values()].some((entry)=>entry.users>0)){watching?.stop();watching=null;}
      };
    },
    subscribe(listener:()=>void){listeners.add(listener);return()=>{listeners.delete(listener);};},
  };
}

export type ScopedPlanBlockRead<T> = ReturnType<typeof createScopedPlanBlockRead<T>>;
