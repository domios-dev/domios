import type { HostIo, HostIoFs, HostIoRequest, HostIoSteps } from "../../../packages/plugin-sdk/src/io";

type Port = Record<string, unknown>;
type Fn = (...args: unknown[]) => unknown;
type TestResult<T> = T extends HostIoSteps<infer R> ? TestResult<R> : T extends (...args: infer A) => infer R ? (...args: A) => TestResult<R> : T extends abstract new (...args: never[]) => infer R ? new (...args: unknown[]) => TestResult<R> : T extends object ? { [K in keyof T]: TestResult<T[K]> } : T;
const parse = (value: unknown): unknown => typeof value === "string" ? JSON.parse(value) : value;
const isProgram = (value: unknown): value is HostIoSteps<unknown> => !!value && typeof (value as HostIoSteps<unknown>).next === "function";

/** Drives authored programs against the suite's deterministic injected ports. */
const fixtureOwners = new WeakMap<object, HostIo>();
const fixtureAdapters = new WeakSet<object>();

export function installTestIo<T extends object>(fixture: T): T & {io: HostIo} {
  const host = fixture as Port;
  if (fixtureOwners.has(fixture) || (host.io && !fixtureAdapters.has(host.io as object))) return fixture as T & {io: HostIo};
  const fs = (host.fs ?? {}) as Port;
  const jobs = new Map<string, {op: "exec" | "fetch"; options: unknown}>();
  const locks = new Set<string>();
  let sequence = 0;
  const invoke = (port: Port, name: string, args: unknown[]): unknown => {
    const fn = port[name];
    if (typeof fn !== "function") throw new Error(`Missing fixture port ${name}`);
    return (fn as Fn).apply(port, args);
  };
  const read = (name: string, args: unknown[]): unknown => {
    if (typeof fs[name] === "function") return invoke(fs, name, args);
    if (typeof host[name] === "function") return invoke(host, name, args);
    if (name === "makeDirs") return true;
    if (name === "lockFile") {
      const key = String(args[0]);
      if (locks.has(key)) return null;
      locks.add(key); return key;
    }
    if (name === "unlockFile") return locks.delete(String(args[0]));
    if (name === "readFile" && typeof fs.readJson === "function") {
      const value = invoke(fs, "readJson", args); return value == null ? null : JSON.stringify(value);
    }
    return invoke(fs, name, args);
  };
  const execute = (request: HostIoRequest): unknown => {
    if (request.op === "execStart" || request.op === "fetchStart") {
      if (typeof host[request.op] === "function") return parse(invoke(host, request.op, [JSON.stringify(request.options)]));
      const id = `fixture-io-${++sequence}`;
      jobs.set(id, {op: request.op === "execStart" ? "exec" : "fetch", options: request.options});
      return {jobId: id};
    }
    if (request.op === "providerVersion") return parse(invoke(host, "providerVersionJson", []));
    if (request.op === "credentialPublic") return parse(invoke(host, "credentialPublic", [request.id]));
    if (request.op === "projectWriteAllowed") return invoke(host, request.op, [request.root]);
    if (request.op === "jsonBooleanRecord") return parse(invoke(host, request.op, [request.path, JSON.stringify(request.keys), request.grant]));
    if (request.op === "sqliteQuery") {
      if (request.nullable && typeof host.sqliteQueryAllJson === "function") return parse(invoke(host, "sqliteQueryAllJson", [request.db, request.sql, JSON.stringify(request.params ?? [])]));
      return invoke(fs, "sqliteQuery", [request.db, request.sql, request.params ?? []]);
    }
    if (request.op === "unlockFile") return read(request.op, [request.id]);
    if (request.op === "writeTempFile") return read(request.op, [request.contents, request.suffix]);
    if ("path" in request) {
      const args: unknown[] = [request.path];
      if ("contents" in request) args.push(request.contents);
      if ("maxBytes" in request) args.push(request.maxBytes);
      if ("maxDepth" in request) args.push(request.maxDepth);
      return read(request.op, args);
    }
    throw new Error(`Unsupported fixture I/O ${request.op}`);
  };
  const request = (effect: HostIoRequest, then: (value: unknown, failed: boolean) => unknown): unknown => {
    if (effect.op === "sqliteQuery" && (!effect.nullable || typeof host.sqliteQueryAllJson !== "function") && typeof fs.sqliteQueryAsync === "function") {
      return invoke(fs, "sqliteQueryAsync", [effect.db, effect.sql, effect.params ?? [], (rows: unknown) => then(rows, false)]);
    }
    if (effect.op === "awaitJob") {
      const job = jobs.get(effect.job);
      if (!job) return invoke(host, "awaitJob", [effect.job, (value: unknown) => then(parse(value), false)]);
      jobs.delete(effect.job);
      const port = host[job.op] as Fn & {async?: (options: unknown, then: Fn) => unknown};
      if (typeof port?.async === "function") return port.async(job.options, value => then(parse(value), false));
      let value: unknown;
      try { value = parse(invoke(host, job.op, [JSON.stringify(job.options)])); }
      catch (error) { return then(error, true); }
      return then(value, false);
    }
    let value: unknown;
    try { value = execute(effect); } catch (error) { return then(error, true); }
    return then(value, false);
  };
  const run = (steps: HostIoSteps<unknown>): unknown => {
    const resume = (value?: unknown, failed = false): unknown => {
      for (;;) {
        const next = failed ? steps.throw(value) : steps.next(value);
        if (next.done) {
          if (!isProgram(next.value)) return next.value;
          steps = next.value; value = undefined; failed = false; continue;
        }
        let inline = true;
        const state: {reply?: {value: unknown; failed: boolean}} = {};
        const pending = request(next.value, (result, rejected) => {
          if (inline) { state.reply = {value: result, failed: rejected}; return undefined; }
          return resume(result, rejected);
        });
        inline = false;
        if (!state.reply) return pending;
        value = state.reply.value; failed = state.reply.failed;
      }
    };
    return resume();
  };
  const io = ((effect: HostIoRequest, then: Fn) => request(effect, (value, failed) => { if (failed) throw value; return then(value); })) as HostIo;
  io.run = run as HostIo["run"];
  io.fs = {} as HostIoFs;
  for (const name of ["readFile", "fileExists", "isSymlink", "canonicalPath", "lockFile", "removeFile", "makeDirs", "readDir", "listChildDirs"] as const) {
    (io.fs as unknown as Port)[name] = function* (path: string): HostIoSteps<unknown> { return yield {op: name, path}; };
  }
  for (const name of ["readFileHead", "readFileTail"] as const) (io.fs as unknown as Port)[name] = function* (path: string, maxBytes: number): HostIoSteps<unknown> { return yield {op: name, path, maxBytes}; };
  for (const name of ["writeFile", "writeFileAtomic"] as const) (io.fs as unknown as Port)[name] = function* (path: string, contents: string): HostIoSteps<unknown> { return yield {op: name, path, contents}; };
  io.fs.unlockFile = function* (id) { return (yield {op: "unlockFile", id}) as boolean; };
  io.fs.writeTempFile = function* (contents, suffix) { return (yield {op: "writeTempFile", contents, suffix}) as string | null; };
  io.fs.walkDir = function* (path, maxDepth = 5) { return (yield {op: "walkDir", path, maxDepth}) as import("../../../packages/plugin-sdk/src/host").DirEntry[]; };
  io.fs.sqliteQuery = function* <R>(db: string, sql: string, params?: unknown[] | null) { return (yield {op: "sqliteQuery", db, sql, params}) as R[]; };
  io.fs.sqliteQueryNullable = function* <R>(db: string, sql: string, params?: unknown[] | null) { return (yield {op: "sqliteQuery", db, sql, params, nullable: true}) as R[] | null; };
  io.fs.readJson = function* <R>(path: string) { return (typeof fs.readJson === "function" ? invoke(fs, "readJson", [path]) : parse(yield {op: "readFile", path})) as R | null; };
  io.fs.writeJson = function* (path, value) { return (typeof fs.writeJson === "function" ? invoke(fs, "writeJson", [path, value]) : yield {op: "writeFile", path, contents: JSON.stringify(value, null, 2)}) as boolean; };
  io.exec = function* (options) { const started = (yield {op: "execStart", options}) as {jobId?: string; error?: string}; return started.jobId ? yield {op: "awaitJob", job: started.jobId} : {error: started.error}; } as HostIo["exec"];
  io.fetch = function* (options) { const started = (yield {op: "fetchStart", options}) as {jobId?: string; error?: string}; return started.jobId ? yield {op: "awaitJob", job: started.jobId} : {error: started.error}; } as HostIo["fetch"];
  host.io = io;
  fixtureOwners.set(fixture, io);
  fixtureAdapters.add(io);
  return fixture as T & {io: HostIo};
}

export function ioTestContext<T extends {host?: object}>(context: T): T {
  if (context.host) installTestIo(context.host);
  return context;
}

export function ioTestExport<T>(source: T): TestResult<T> {
  const wrap = (value: unknown, capturedHost?: object): unknown => {
    if (typeof value === "function") return new Proxy(value, {
      apply(fn, receiver, args) {
        const globalHost = (globalThis as unknown as {host?: object}).host;
        if (globalHost) installTestIo(globalHost);
        for (const arg of args) if (arg && typeof arg === "object" && (["fs", "io", "execStart", "credentialPublic", "canonicalPath", "jsonBooleanRecord"].some(key => key in arg))) installTestIo(arg);
        const prepared = args.map(arg => typeof arg === "function" ? (...inner: unknown[]) => {
          const result: unknown = arg(...inner);
          if (result && typeof result === "object" && "manifestJson" in result) installTestIo(result);
          return result;
        } : arg);
        const result: unknown = Reflect.apply(fn, receiver, prepared);
        const host = (args.find(arg => arg && typeof arg === "object" && "io" in arg) ?? capturedHost ?? globalHost ?? {}) as object;
        return wrap(isProgram(result) ? installTestIo(host).io.run(result) : result, host);
      },
      construct(fn, args) {
        let host = capturedHost;
        if (fn.name === "RemoteTaskFileStorage" && args[0]) { host = installTestIo({fs: args[0]}); args[0] = (host as {io: HostIo}).io.fs; }
        return wrap(Reflect.construct(fn, args), host) as object;
      },
    });
    if (!value || typeof value !== "object" || Array.isArray(value) || value instanceof Map || value instanceof Set) return value;
    // Object.prototype builtins stay unwrapped: deep equality compares them by identity.
    const builtin = (key: PropertyKey, property: unknown) => (Object.prototype as unknown as Record<PropertyKey, unknown>)[key] === property;
    return new Proxy(value, { get(object, key) { const property: unknown = Reflect.get(object, key); return typeof property === "function" && !builtin(key, property) ? (wrap(property, capturedHost) as Fn).bind(object) : key === "default" ? wrap(property, capturedHost) : property; } });
  };
  return wrap(source) as TestResult<T>;
}
