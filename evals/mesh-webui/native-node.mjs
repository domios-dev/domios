import assert from 'node:assert/strict';
import {readFile,realpath} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import path from 'node:path';

const LIMIT=16*1024*1024;
export function nativeBlocks(value) {
  if(typeof value==='string') return value;
  if(Array.isArray(value)) return value.map(item=>item?.text??'').join('');
  return value?.text??'';
}
export function parseNativeJsonl(provider,sid,source,rows,sessionModel=null) {
  assert.ok(sid);
  const records=[];
  if(provider==='codex') {
    const metas=rows.filter(([,raw])=>raw.type==='session_meta');
    assert.equal(metas.length,1);assert.equal(metas[0][1].payload.id,sid);
  } else if(provider==='claude') assert.ok(rows.some(([,raw])=>raw.sessionId===sid));
  let model=null;
  for(const [line,raw] of rows) {
    let role='',text='',tool=false,attachment=false,cancelled=false;
    if(provider==='codex') {
      const payload=raw.payload??{};
      if(raw.type==='turn_context') model=payload.model;
      role=raw.type==='response_item' ? payload.role??'' : '';
      text=nativeBlocks(payload.content);
      tool=['function_call','custom_tool_call'].includes(payload.type);
      attachment=Array.isArray(payload.content)&&payload.content.some(c=>c.type==='input_image');
      cancelled=raw.type==='event_msg'&&payload.type==='turn_aborted';
    } else if(provider==='claude') {
      if(raw.sessionId!==sid||raw.isSidechain===true) continue;
      const message=raw.message??{},content=message.content??raw.content??'';
      role=raw.type??'';text=nativeBlocks(content);model=message.model??null;
      tool=Array.isArray(content)&&content.some(c=>c.type==='tool_use');
      attachment=Array.isArray(content)&&content.some(c=>['image','document'].includes(c.type));
      cancelled=raw.type==='user'&&['[Request interrupted by user]','[Request interrupted by user for tool use]'].includes(text);
    } else if(provider==='grok') {
      const update=raw.params?.update??{},kind=update.sessionUpdate;
      if(kind==='user_message_chunk') model=update._meta?.modelId??null;
      role=({user_message_chunk:'user',agent_message_chunk:'assistant'})[kind]??'';
      text=nativeBlocks(update.content);tool=kind==='tool_call';
      attachment=['image','resource'].includes(update.content?.type);
      cancelled=kind==='prompt_response'&&update.stopReason==='cancelled';
    } else throw Error('Unsupported JSONL provider');
    records.push({raw,source_path:source,locator:'line:'+line,role,text,model,tool,attachment,cancelled});
  }
  return {provider,session_id:sid,records,session_model:sessionModel};
}
async function bytes(filename) {
  const name=await realpath(filename),data=await readFile(name);
  assert.ok(data.length<=LIMIT,'Native file exceeds16MiB');
  return {name,data,proof:{path:name,sha256:createHash('sha256').update(data).digest('hex'),content:data.toString('utf8')}};
}
export async function exportNative(spec) {
  assert.ok(spec.session_id);
  if(spec.provider==='opencode') {
    const {DatabaseSync}=await import('node:sqlite');
    const filename=await realpath(spec.path),db=new DatabaseSync(filename,{readOnly:true});
    try {
      db.exec('BEGIN');
      const session=db.prepare('SELECT * FROM session WHERE id = ?').get(spec.session_id);
      assert.ok(session,'OpenCode exact session absent');
      const messages=db.prepare('SELECT id, session_id, time_created, data FROM message WHERE session_id = ? ORDER BY time_created, id').all(spec.session_id);
      const records=[];let size=0;
      for(const row of messages) {
        const message=JSON.parse(row.data);
        const parts=db.prepare('SELECT id, session_id, message_id, data FROM part WHERE session_id = ? AND message_id = ? ORDER BY time_created, id').all(spec.session_id,row.id);
        const parsed=parts.map(p=>JSON.parse(p.data));
        size+=Buffer.byteLength(row.data)+parts.reduce((n,p)=>n+Buffer.byteLength(p.data),0);
        assert.ok(size<=LIMIT,'OpenCode exact rows exceed16MiB');
        records.push({raw:{...row,parts},source_path:filename,locator:'message.id:'+row.id,
          role:message.role??'',text:parsed.filter(p=>p.type==='text').map(p=>p.text??'').join(''),
          model:message.providerID&&message.modelID ? message.providerID+'/'+message.modelID : null,
          tool:parsed.some(p=>p.type==='tool'),attachment:parsed.some(p=>p.type==='file'),
          cancelled:message.error?.name==='MessageAbortedError'});
      }
      db.exec('ROLLBACK');
      return {provider:spec.provider,session_id:spec.session_id,records,sources:[{path:filename,table:'session',row:session}],session_model:null};
    } finally {db.close();}
  }
  const source=await bytes(spec.path),sources=[];let sessionModel=null;
  if(spec.provider==='grok') {
    const summary=await bytes(spec.summary_path),value=JSON.parse(summary.data);
    assert.equal(value.info?.id,spec.session_id);
    assert.equal(path.dirname(source.name),path.dirname(summary.name));
    assert.equal(path.basename(path.dirname(source.name)),spec.session_id);
    assert.equal(path.basename(source.name),'updates.jsonl');
    sessionModel=value.current_model_id??null;sources.push(summary.proof);
  }
  const rows=source.data.toString('utf8').split(/\r?\n/).map((line,i)=>[i+1,line]).filter(([,line])=>line.trim()).map(([i,line])=>[i,JSON.parse(line)]);
  sources.push(source.proof);
  return {...parseNativeJsonl(spec.provider,spec.session_id,source.name,rows,sessionModel),sources};
}
