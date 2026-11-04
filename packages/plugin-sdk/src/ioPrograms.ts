import type { HostIo, HostIoRequest, HostIoSteps } from "./io";

export function* ioValue<T>(request: HostIoRequest): HostIoSteps<T> {
  return (yield request) as T;
}

export function* ioMap<T, R>(values: readonly T[], project: (value: T, index: number) => HostIoSteps<R>): HostIoSteps<R[]> {
  const result: R[] = [];
  for (let index = 0; index < values.length; index++) result.push(yield* project(values[index], index));
  return result;
}

export function* ioFilter<T>(values: readonly T[], predicate: (value: T, index: number) => HostIoSteps<unknown>): HostIoSteps<T[]> {
  const result: T[] = [];
  for (let index = 0; index < values.length; index++) if (yield* predicate(values[index], index)) result.push(values[index]);
  return result;
}

export type IoExports<T> = { [K in keyof T]: NonNullable<T[K]> extends (...args: infer A) => infer R ? (...args: A) => HostIoSteps<R> | R : T[K] };
export type IoResults<T> = { [K in keyof T]: T[K] extends (...args: infer A) => infer R ? (...args: A) => R extends HostIoSteps<infer V> ? V : R : T[K] };

/** Wrap exports once; each invocation resumes its original iterator without replay. */
export function defineProviderIo<T extends object>(source: T, io: () => HostIo): IoResults<T> {
  const exports = { ...source } as Record<string, unknown>;
  for (const key of Object.keys(source)) {
    const value = (source as Record<string, unknown>)[key];
    if (typeof value !== "function") continue;
    exports[key] = function (...args: unknown[]) {
      const result: unknown = value.apply(source, args);
      return result && typeof (result as HostIoSteps<unknown>).next === "function"
        ? io().run(result as HostIoSteps<unknown>) : result;
    };
  }
  return exports as IoResults<T>;
}

export type { HostIoSteps } from "./io";

export function* ioEncoded(steps: HostIoSteps<unknown>): HostIoSteps<string> {
  return JSON.stringify(yield* steps);
}

export function ioJson(request: HostIoRequest): HostIoSteps<string>;
export function ioJson(request: HostIoRequest, nullable: true): HostIoSteps<string | null>;
export function* ioJson(request: HostIoRequest, nullable = false): HostIoSteps<string | null> {
  const value = yield* ioValue<unknown>(request);
  return nullable && value == null ? null : JSON.stringify(value);
}

export function* ioResolve<T>(value: T | HostIoSteps<T>): HostIoSteps<T> {
  if (value && typeof (value as HostIoSteps<T>).next === "function") return yield* value as HostIoSteps<T>;
  return value as T;
}

export function* ioSqlite<T, R = T[]>(fs: import("./io").HostIoFs, db: string, sql: string, params: unknown[], then: (rows: T[]) => R | HostIoSteps<R>): HostIoSteps<R> {
  return yield* ioResolve(then(yield* fs.sqliteQuery<T>(db, sql, params)));
}

/** Locks stay owned by the invocation while its iterator is suspended. */
export function* ioFileTransaction<T>(fs: import("./io").HostIoFs, paths: string[], work: () => HostIoSteps<T>): HostIoSteps<T | {ok: false; error: string}> {
  const locks: string[] = [];
  try {
    for (const path of [...new Set(paths)].sort()) {
      const parent = path.slice(0, Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")));
      if (parent && !(yield* fs.makeDirs(parent))) return {ok: false, error: "integration directory is unavailable"};
      const lock = yield* fs.lockFile(path);
      if (!lock) return {ok: false, error: "integration files are busy"};
      locks.push(lock);
    }
    return yield* work();
  } finally {
    for (const lock of locks.reverse()) yield* fs.unlockFile(lock);
  }
}

export function* ioThen<T, R>(steps: HostIoSteps<T>, then: (value: T) => R | HostIoSteps<R>): HostIoSteps<R> {
  return yield* ioResolve(then(yield* steps));
}
