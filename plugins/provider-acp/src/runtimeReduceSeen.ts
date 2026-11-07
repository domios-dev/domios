// A fixed-size filter of message identities seen in the active turn. A false positive only gives a
// new message a continuation row id; it never lets a resumed message overwrite an earlier row.
const BITS=4096;

function hash(value:string, seed:number):number {
  let hash=seed;
  for (let index=0;index<value.length;index++) {
    hash^=value.charCodeAt(index);
    hash=Math.imul(hash,16777619)>>>0;
  }
  return hash%BITS;
}

/** Records `id` in `filter` and reports whether it may have been recorded before. */
export function markSeen(filter:string|undefined, id:string):{filter:string; seen:boolean} {
  const bytes=new Uint8Array(BITS/8);
  if (filter?.length === BITS/4) for (let index=0;index<bytes.length;index++) bytes[index]=Number.parseInt(filter.slice(index*2,index*2+2),16);
  let seen=true;
  for (const bit of [hash(id,2166136261),hash(id,374761393)]) {
    const mask=1<<(bit%8);
    const byte=bytes[bit>>3] ?? 0;
    if (!(byte & mask)) seen=false;
    bytes[bit>>3]=byte|mask;
  }
  return {filter:Array.from(bytes,(byte)=>byte.toString(16).padStart(2,"0")).join(""),seen};
}
