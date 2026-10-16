import {requireValue} from './model.mjs';

export function snapshotGraph(snapshot) {
  const {node_fields:nf, edge_fields:ef, edge_types:et} = snapshot.snapshot.meta;
  const n = snapshot.nodes, e = snapshot.edges, strings = snapshot.strings;
  for (const f of ['name','self_size','edge_count']) requireValue(nf.includes(f), `Missing node ${f}`);
  for (const f of ['type','name_or_index','to_node']) requireValue(ef.includes(f), `Missing edge ${f}`);
  requireValue(n.length%nf.length === 0 && e.length%ef.length === 0, 'Malformed heap strides');
  const nodes = [];
  let cursor = 0;
  for (let offset = 0; offset < n.length; offset += nf.length) {
    const edges = [], count = n[offset+nf.indexOf('edge_count')];
    for (let i = 0; i < count; i++,cursor += ef.length) {
      const type = et[ef.indexOf('type')][e[cursor+ef.indexOf('type')]];
      const raw = e[cursor+ef.indexOf('name_or_index')], to = e[cursor+ef.indexOf('to_node')]/nf.length;
      requireValue(Number.isInteger(to) && to >= 0 && to < n.length/nf.length, 'Invalid heap target');
      if (type !== 'weak') edges.push({to,name:type === 'element' || type === 'hidden' ? `${type}[${raw}]` : `${type}:${strings[raw]}`});
    }
    nodes.push({name:strings[n[offset+nf.indexOf('name')]],bytes:n[offset+nf.indexOf('self_size')],edges});
  }
  requireValue(cursor === e.length, 'Heap edge count mismatch');
  return nodes;
}

export function analyzeGraph(nodes) {
  requireValue(nodes.length > 0, 'Empty heap graph');
  const predecessors = nodes.map(() => []), parent = new Int32Array(nodes.length).fill(-1), via = [];
  const order = [0]; parent[0] = 0;
  for (let q = 0; q < order.length; q++) {
    const from = order[q];
    for (const edge of nodes[from].edges) {
      requireValue(nodes[edge.to] !== undefined, 'Invalid graph target');
      predecessors[edge.to].push(from);
      if (parent[edge.to] < 0) { parent[edge.to] = from; via[edge.to] = edge.name; order.push(edge.to); }
    }
  }
  const seen = new Set([0]), stack = [[0,0]], post = [];
  while (stack.length) {
    const frame = stack.at(-1), edges = nodes[frame[0]].edges;
    if (frame[1] === edges.length) { post.push(frame[0]); stack.pop(); continue; }
    const to = edges[frame[1]++].to;
    if (!seen.has(to)) { seen.add(to); stack.push([to,0]); }
  }
  const rpo = post.reverse(), rank = new Int32Array(nodes.length), idom = new Int32Array(nodes.length).fill(-1);
  rpo.forEach((id,i) => { rank[id] = i; }); idom[0] = 0;
  const intersect = (a,b) => {
    while (a !== b) { while (rank[a] > rank[b]) a = idom[a]; while (rank[b] > rank[a]) b = idom[b]; }
    return a;
  };
  let changed = true;
  while (changed) {
    changed = false;
    for (const id of rpo.slice(1)) {
      const preds = predecessors[id].filter(p => idom[p] >= 0);
      const next = preds.slice(1).reduce(intersect,preds[0]);
      if (idom[id] !== next) { idom[id] = next; changed = true; }
    }
  }
  const retained = nodes.map(n => n.bytes);
  for (const id of [...rpo].reverse()) if (id !== 0) retained[idom[id]] += retained[id];
  const path = id => {
    const parts = [];
    for (let count = 0; id !== 0 && count < 24; count++,id = parent[id]) parts.push(`${via[id]}→${nodes[id].name}`);
    if (id !== 0) parts.push('…');
    return [nodes[0].name,...parts.reverse()].join('/');
  };
  const constructors = new Map(), paths = new Map();
  for (const id of order) {
    const node = nodes[id], retainingPath = path(id), dominatorPath = path(idom[id]);
    for (const [map,key] of [[constructors,node.name],[paths,JSON.stringify([node.name,retainingPath,dominatorPath])]]) {
      const row = map.get(key) ?? {constructor:node.name,count:0,selfBytes:0,retainedBytes:0,
        ...(map === paths ? {retainingPath,dominatorPath} : {})};
      row.count++; row.selfBytes += node.bytes; row.retainedBytes += retained[id]; map.set(key,row);
    }
  }
  return {constructors,paths,idom:[...idom],retained,unreachable:nodes.length-order.length};
}
export function heapDiff(before,after) {
  const diff = field => [...new Set([...before[field].keys(),...after[field].keys()])].map(key => {
    const a = before[field].get(key), b = after[field].get(key);
    return {...(b??a),count:(b?.count??0)-(a?.count??0),selfBytes:(b?.selfBytes??0)-(a?.selfBytes??0),
      retainedBytes:(b?.retainedBytes??0)-(a?.retainedBytes??0)};
  }).sort((a,b) => b.selfBytes-a.selfBytes);
  return {constructors:diff('constructors'),retainingPaths:diff('paths'),unreachable:{before:before.unreachable,after:after.unreachable},
    retainedBytesNote:'Summed per-object retained sizes overlap; selfBytes are additive. Weak edges excluded.'};
}
