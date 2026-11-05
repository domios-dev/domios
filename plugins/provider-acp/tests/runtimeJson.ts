export function runtimeJson<T>(value:T, hook:string):T {
  const paths=new WeakMap<object,string>();
  const encoded=JSON.stringify(value,function(this:unknown,key:string,entry:unknown):unknown {
    const parent=this !== null && typeof this === "object" ? paths.get(this) ?? "$" : "$";
    const path=key ? `${parent}.${key}` : "$";
    if (entry === undefined || typeof entry === "function" || typeof entry === "symbol"
      || typeof entry === "bigint" || typeof entry === "number" && !Number.isFinite(entry))
      throw new Error(`${hook}: non-JSON ${typeof entry} at ${path}`);
    if (entry !== null && typeof entry === "object") paths.set(entry,path);
    return entry;
  });
  if (typeof encoded !== "string") throw new Error(`${hook}: returned no JSON`);
  return value;
}
