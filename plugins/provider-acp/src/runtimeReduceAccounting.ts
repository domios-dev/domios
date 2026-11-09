export const mapKeys=["items","controls","warnings","offsetById"] as const;
type MapKey=typeof mapKeys[number];
type Maps=Record<MapKey,Record<string,unknown>>;
type Totals=Record<MapKey,number>;
// Keep byte totals outside serialized state so they do not change eviction thresholds.
const totalsByState=new WeakMap<object,Totals>();
export function utf8Length(value:string):number {
  let bytes=0;
  for (let index=0;index<value.length;) {
    const code=value.codePointAt(index)!;
    bytes+=code<=127 ? 1 : code<=2047 ? 2 : code<=65535 ? 3 : 4;
    index+=code>65535 ? 2 : 1;
  }
  return bytes;
}
function contribution(id:string, value:unknown):number {
  if (value === undefined || typeof value === "function" || typeof value === "symbol") return 0;
  return utf8Length(JSON.stringify({[id]:value}))-1;
}
function totals(state:Maps):Totals {
  let cached=totalsByState.get(state);
  if (!cached) {
    cached={items:0,controls:0,warnings:0,offsetById:0};
    for (const key of mapKeys) for (const [id,value] of Object.entries(state[key])) cached[key]+=contribution(id,value);
    totalsByState.set(state,cached);
  }
  return cached;
}
export function copyAccounting(source:Maps, target:Maps):void {
  totalsByState.set(target,{...totals(source)});
}
export function forgetAccounting(state:unknown):void {
  if (state !== null && typeof state === "object") totalsByState.delete(state);
}
export function mapBytes(state:Maps, key:MapKey):number {
  const total=totals(state)[key];
  return total ? total+1 : 2;
}
export function putEntry(state:Maps, key:MapKey, id:string, value:unknown):void {
  const cached=totals(state);
  cached[key]+=contribution(id,value)-contribution(id,state[key][id]);
  state[key][id]=value;
}
export function removeEntry(state:Maps, key:MapKey, id:string):number {
  const before=mapBytes(state,key), cached=totals(state);
  cached[key]-=contribution(id,state[key][id]);
  delete state[key][id];
  return mapBytes(state,key)-before;
}
export function clearEntries(state:Maps, key:MapKey):void {
  totals(state)[key]=0;
  state[key]=Object.create(null);
}
