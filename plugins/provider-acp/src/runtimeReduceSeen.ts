// A fixed-size filter of message identities whose offsets were evicted in the active turn. A false
// positive only gives a new message a continuation row id; it never lets a resumed message overwrite an
// earlier row. It stays absent until the first eviction, so ordinary turns carry no extra state.
const BITS=4096;

function bits(id:string):number[] {
  return [2166136261,374761393].map((seed)=>{
    let hash=seed;
    for (let index=0;index<id.length;index++) {
      hash^=id.charCodeAt(index);
      hash=Math.imul(hash,16777619)>>>0;
    }
    return hash%BITS;
  });
}

function decode(filter:string|undefined):Uint8Array {
  const bytes=new Uint8Array(BITS/8);
  if (filter?.length === BITS/4) for (let index=0;index<bytes.length;index++) bytes[index]=Number.parseInt(filter.slice(index*2,index*2+2),16);
  return bytes;
}

export function evictedMayInclude(filter:string|undefined, id:string):boolean {
  if (filter === undefined) return false;
  const bytes=decode(filter);
  return bits(id).every((bit)=>((bytes[bit>>3] ?? 0) & (1<<(bit%8))) !== 0);
}

export function recordEvicted(filter:string|undefined, ids:readonly string[]):string|undefined {
  if (!ids.length) return filter;
  const bytes=decode(filter);
  for (const id of ids) for (const bit of bits(id)) bytes[bit>>3]=(bytes[bit>>3] ?? 0)|(1<<(bit%8));
  return Array.from(bytes,(byte)=>byte.toString(16).padStart(2,"0")).join("");
}
