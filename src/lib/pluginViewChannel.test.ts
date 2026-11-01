import { afterEach, describe, expect, it, vi } from "vitest";
import { PluginViewChannel, type FrameIntent } from "./pluginViewChannel";
import { TestViewPort } from "./pluginViewChannel.testSupport";
const live: PluginViewChannel[] = [];
afterEach(() => { for (const channel of live.splice(0)) channel.dispose(); });
function fixture() {
  const channel = new PluginViewChannel(crypto.randomUUID().replaceAll("-", ""));live.push(channel);
  const source = {} as Window;
  let intent: FrameIntent = {shown:true,frameFocused:true,activated:true,generation:1};
  const receive = vi.fn();const ready=vi.fn();const invalid=vi.fn();
  channel.bind(source, () => ({...intent}));
  channel.subscribe(receive, ready, invalid);
  return {channel,source,receive,ready,invalid,setIntent:(value:FrameIntent)=>{intent=value;}};
}

describe("document-bound plugin view channel", () => {
  it("preserves ordinary initial reads before first load and supports an early hello before binding", () => {
    const channel=new PluginViewChannel(crypto.randomUUID().replaceAll("-", ""));live.push(channel);
    const source={} as Window;const port=new TestViewPort();const receive=vi.fn();
    channel.accept(source,port.port);
    channel.bind(source,()=>({shown:false,frameFocused:false,activated:false,generation:0}));
    port.receive({__ctInvoke:1,method:"initialRead"});
    channel.subscribe(receive,vi.fn(),vi.fn());
    expect(receive).not.toHaveBeenCalled();port.prove();
    expect(receive).toHaveBeenCalledWith({__ctInvoke:1,method:"initialRead"},expect.any(Object));
  });

  it("does not authorize a replacement navigating before the first observed load or expose old results", () => {
    const f=fixture();const original=new TestViewPort();
    f.channel.accept(f.source,original.port);original.prove();
    original.receive({__ctInvoke:1,method:"read"});
    expect(f.receive).toHaveBeenCalledTimes(1);
    // Navigation keeps WindowProxy but destroys the original document endpoint.
    f.channel.observeLoad();
    f.channel.send({__ctReply:1,result:{private:true}});
    window.dispatchEvent(new MessageEvent("message",{source:f.source,data:{__ctInvoke:2,method:"submit"}}));
    window.dispatchEvent(new MessageEvent("message",{source:f.source,data:{__ctProof:2}}));
    expect(f.receive).toHaveBeenCalledTimes(1);
    expect(original.sent.some(message=>message.__ctReply===1)).toBe(false);
    const replacement=new TestViewPort();f.channel.accept(f.source,replacement.port);
    expect(f.invalid).toHaveBeenCalledExactlyOnceWith();
    expect(original.closed).toBe(true);expect(replacement.closed).toBe(true);
    expect(replacement.sent).toEqual([]);
  });

  it("rejects foreign hello ports without replacing the original endpoint", () => {
    const f=fixture();const foreign=new TestViewPort();
    window.dispatchEvent(new MessageEvent("message",{source:{} as Window,ports:[foreign.port],data:{__ctReady:f.channel.id}}));
    expect(foreign.closed).toBe(true);
    const own=new TestViewPort();
    window.dispatchEvent(new MessageEvent("message",{source:f.source,ports:[own.port],data:{__ctReady:f.channel.id}}));
    own.prove();
    own.receive({__ctInvoke:1,method:"read"});expect(f.receive).toHaveBeenCalledTimes(1);
    expect(f.invalid).not.toHaveBeenCalled();
  });

  it("captures gesture intent at receipt before proof and drops stale challenges", () => {
    const f=fixture();const port=new TestViewPort();f.channel.accept(f.source,port.port);
    port.receive({__ctHost:1,action:"copyText",args:{text:"opaque"}});
    f.setIntent({shown:false,frameFocused:false,activated:false,generation:2});
    f.channel.observeLoad();port.receive({__ctProof:1});expect(f.receive).not.toHaveBeenCalled();
    port.prove();
    expect(f.receive).toHaveBeenCalledWith(expect.objectContaining({__ctHost:1}),{shown:true,frameFocused:true,activated:true,generation:1});
  });

  it("queues host support and replies on the private port and reproves every load", () => {
    const f=fixture();const port=new TestViewPort();f.channel.accept(f.source,port.port);
    f.channel.send({__ctHostSupport:{copyText:true}});expect(port.sent).toHaveLength(1);
    port.prove();expect(port.sent).toContainEqual({__ctHostSupport:{copyText:true}});
    f.channel.observeLoad();f.channel.send({__ctReply:1,result:"opaque"});
    expect(port.sent.some(message=>message.__ctReply===1)).toBe(false);
    port.prove();expect(port.sent).toContainEqual({__ctReply:1,result:"opaque"});
  });

  it("bounds queued requests, detects malformed messages and closes on disposal", () => {
    for (const attack of ["count","bytes","shape"] as const) {
      const f=fixture();const port=new TestViewPort();f.channel.accept(f.source,port.port);
      if(attack==="count") for(let i=0;i<33;i++)port.receive({__ctInvoke:i,method:"read"});
      else if(attack==="bytes")port.receive({args:"x".repeat(256*1024)});
      else port.receive([]);
      expect(f.invalid).toHaveBeenCalledTimes(1);expect(port.closed).toBe(true);
      port.prove();expect(f.receive).not.toHaveBeenCalled();
    }
  });

  it("unsubscribe keeps document lifetime but disposal rejects reuse and reopening needs a new identity", () => {
    const f=fixture();const port=new TestViewPort();f.channel.accept(f.source,port.port);port.prove();
    const receiver=vi.fn();const release=f.channel.subscribe(receiver,vi.fn(),vi.fn());release();
    port.receive({__ctInvoke:1,method:"read"});expect(receiver).not.toHaveBeenCalled();
    f.channel.subscribe(receiver,vi.fn(),vi.fn());expect(receiver).toHaveBeenCalledTimes(1);
    f.channel.dispose();port.receive({__ctInvoke:2,method:"read"});expect(receiver).toHaveBeenCalledTimes(1);
    const replacement=new TestViewPort();f.channel.accept(f.source,replacement.port);expect(replacement.closed).toBe(true);
    const reopened=fixture();expect(reopened.channel.id).not.toBe(f.channel.id);
  });
});

it.each(["bytes", "cycle", "bigint", "array", "sparse", "depth"] as const)("refuses proven-channel %s envelopes before receiver/effects", (attack) => {
  const f=fixture();const port=new TestViewPort();f.channel.accept(f.source,port.port);port.prove();
  const args: Record<string, unknown>={};
  if(attack==="bytes") args.text="x".repeat(256*1024);
  if(attack==="cycle") args.self=args;
  if(attack==="bigint") args.number=1n;
  if(attack==="array") args.array=new Array(1000000);
  if(attack==="sparse") args.array=Array.from({length:128},()=>new Array(128));
  if(attack==="depth") {let nested=args;for(let i=0;i<18;i++){const child={};nested.child=child;nested=child;}}
  port.receive({__ctInvoke:1,method:"submit",args});
  expect(f.receive).not.toHaveBeenCalled();expect(f.invalid).toHaveBeenCalledTimes(1);
});

it("retires before challenging and registers a fresh backend authority before dispatch", async () => {
  let finishRetire!:()=>void;let finishRegister!:()=>void;
  const retired=new Promise<void>(resolve=>{finishRetire=resolve;});const registered=new Promise<void>(resolve=>{finishRegister=resolve;});
  const retire=vi.fn(()=>retired);const register=vi.fn(()=>registered);
  const channel=new PluginViewChannel(crypto.randomUUID().replaceAll("-", ""),{retire,register});live.push(channel);
  const source={} as Window;const port=new TestViewPort();const receive=vi.fn();
  channel.bind(source,()=>({shown:true,frameFocused:true,activated:true,generation:0}));
  channel.subscribe(receive,vi.fn(),vi.fn());channel.accept(source,port.port);
  expect(retire).toHaveBeenCalledTimes(1);expect(port.sent).toEqual([]);
  port.receive({__ctProof:1});expect(register).not.toHaveBeenCalled();
  port.receive({__ctInvoke:1,method:"read"});expect(receive).not.toHaveBeenCalled();
  finishRetire();await retired;await Promise.resolve();port.prove();
  expect(register).toHaveBeenCalledTimes(1);expect(receive).not.toHaveBeenCalled();
  finishRegister();await registered;await Promise.resolve();
  expect(receive).toHaveBeenCalledTimes(1);
});

it("a failed retire or fresh registration never admits queued effects", async () => {
  for(const failing of ["retire","register"] as const){
    const channel=new PluginViewChannel(crypto.randomUUID().replaceAll("-", ""),{
      retire:()=>failing==="retire"?Promise.reject(new Error("refused")):Promise.resolve(),
      register:()=>Promise.reject(new Error("refused")),
    });live.push(channel);
    const source={} as Window;const port=new TestViewPort();const receive=vi.fn();const invalid=vi.fn();
    channel.bind(source,()=>({shown:false,frameFocused:false,activated:false,generation:0}));
    channel.subscribe(receive,vi.fn(),invalid);channel.accept(source,port.port);
    port.receive({__ctInvoke:1,method:"submit"});
    await Promise.resolve();port.prove();await Promise.resolve();
    expect(receive).not.toHaveBeenCalled();expect(invalid).toHaveBeenCalledTimes(1);
  }
});
