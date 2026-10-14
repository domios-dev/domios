import {readFileSync, appendFileSync} from 'node:fs';
import {createInterface} from 'node:readline';
import {fileURLToPath} from 'node:url';
import {leaves} from './model.mjs';

function item(row) {
  if (row.type === 'user') return {id: row.id, type: 'userMessage', content: [
    {type: 'text', text: row.content ?? ''}, ...(row.attachments ?? []).map(attachment => ({
      type: 'image', url: `data:${attachment.mimeType ?? 'image/png'};base64,${attachment.data ?? 'eA=='}`,
    })),
  ]};
  if (row.type === 'thinking') return {id: row.id, type: 'reasoning', summary: [row.content ?? '']};
  if (row.type === 'tool_call') return {id: row.id, type: 'dynamicToolCall', tool: row.toolName ?? 'fixture',
    arguments: row.toolInput ?? {}, result: row.toolResult ?? row.content ?? ''};
  if (row.type === 'tool_result') return {id: row.id, type: 'functionCallOutput', output: row.toolResult ?? row.content ?? ''};
  if (row.type === 'system') return {id: row.id, type: 'error', message: row.content ?? ''};
  if (row.type === 'tool_flow') return {id: row.id, type: 'toolFlow', state: row.content ?? ''};
  if (row.type === 'custom') return {id: row.id, type: 'sessionIntent', objective: row.sessionIntent?.goal ?? ''};
  if (row.type === 'assistant') return {id: row.id, type: 'agentMessage', text: row.content ?? ''};
  throw new Error(`History item ${row.id} has no fake backing for type ${row.type}`);
}

export function peer(document) {
  const histories = new Map(document.histories.map(history => [history.tab_id, history.messages]));
  const threads = new Map(leaves(document).filter(tab => tab.agent_type).map(tab => {
    const groups = new Map();
    for (const row of histories.get(tab.tab_id) ?? []) {
      const turn = row.toolFlowId ?? 'fixture-turn';
      const rows = groups.get(turn) ?? [];
      rows.push(item(row));
      groups.set(turn, rows);
    }
    return [tab.claude_session_id ?? tab.agent_session_id, [...groups].map(([id, items]) => ({id, status: 'completed', items}))];
  }));
  function page(values, cursor, limit) {
    const start = Number(cursor ?? 0);
    if (!Number.isSafeInteger(start) || start < 0) throw new Error('Invalid fixture cursor');
    const end = start + Math.min(limit ?? 100, 100);
    return {data: values.slice(start, end), nextCursor: end < values.length ? String(end) : null};
  }
  return request => {
    const params = request.params ?? {};
    if (request.method === 'initialize') return {userAgent: 'upgrade-restore-fixture'};
    if (request.method === 'account/read') return {account: {type: 'chatgpt', planType: 'fixture'}, requiresOpenaiAuth: false};
    if (request.method === 'model/list') return {data: [], nextCursor: null};
    const turns = threads.get(params.threadId);
    if (!turns) throw new Error(`Fixture has no thread ${params.threadId}`);
    switch (request.method) {
      case 'thread/resume': case 'thread/read':
        return {thread: {id: params.threadId, source: 'cli', threadSource: 'user',
          turns: params.includeTurns ? turns : []}, model: params.model ?? 'fixture-model',
          cwd: params.cwd, approvalPolicy: params.approvalPolicy, sandbox: params.sandbox};
      case 'thread/turns/list': return page(turns.map(({items, ...turn}) => turn), params.cursor, params.limit);
      case 'thread/items/list': {
        const turn = turns.find(turn => turn.id === params.turnId);
        if (!turn) throw new Error(`Fixture has no turn ${params.turnId}`);
        return page(turn.items, params.cursor, params.limit);
      }
      case 'thread/goal/get': return {goal: null};
      default: throw new Error(`Unsupported fixture method ${request.method}`);
    }
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [documentPath, tracePath, ...args] = process.argv.slice(2);
  if (args.includes('--version')) {
    console.log('codex-cli 0.159.0 upgrade-restore-fixture');
  } else {
    if (!args.includes('app-server')) throw new Error('Fixture supports app-server only');
    const respond = peer(JSON.parse(readFileSync(documentPath, 'utf8')));
    for await (const line of createInterface({input: process.stdin, terminal: false})) {
      const request = JSON.parse(line);
      if (request.id == null) continue;
      let response;
      try { response = {id: request.id, result: respond(request)}; }
      catch (error) { response = {id: request.id, error: {code: -32601, message: error.message}}; }
      appendFileSync(tracePath, JSON.stringify({method: request.method, thread_id: request.params?.threadId,
        response_bytes: Buffer.byteLength(JSON.stringify(response)), items: response.result?.data?.length ?? 0}) + '\n');
      process.stdout.write(JSON.stringify(response) + '\n');
    }
  }
}
