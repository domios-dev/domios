module.exports = {
  runtimeStoredHistoryAvailable(cwd, session) { return session !== 'reference-session'; },
  runtimeStoredHistory(session, records) {
    const value = JSON.parse(records);
    if (value.session !== session) throw new Error('foreign session');
    return value.messages;
  },
  runtimePlan(ctx, op) {
    if (op.kind === 'answerQuestion') return {action:{kind:'questionAnswer',input:{text:op.answers.flatMap(answer=>answer.values).join('\n'),attachments:[]}},state:null};
    if (op.kind === 'answer') return {action:{kind:'reply',requestId:op.requestId,result:{kind:'success',value:op.answer}},state:null};
    if (op.kind === 'backgroundProcesses') return {action:{kind:'request',call:{method:op.kind,params:op,target:{threadId:op.threadId}}},state:null};
    if (op.kind === 'interrupt') return {action:{kind:'request',call:{method:op.kind,params:op,target:{threadId:op.threadId,...(op.activeTurnId ? {turnId:op.activeTurnId} : {})}}},state:null};
    if (op.kind === 'forkThread') {
      if (ctx.threadId || ctx.activeTurnId || ctx.activeTurns.length) throw Object.assign(new Error('Fork requires an unbound child runtime'),{code:'binding_mismatch'});
      return {action:{kind:'request',call:{method:'thread/turns/list',params:{threadId:op.sourceThreadId,itemsView:'notLoaded',sortDirection:'asc',limit:8}}},state:{stage:'threadHistoryTurns',action:'fork',sourceThreadId:op.sourceThreadId,turnId:op.turnId,cursor:null,targetFound:false}};
    }
    return {action:{kind:'request',call:{method:op.kind,params:op}},state:null};
  },
  runtimeResponse(_ctx, op, state, reply) {
    if (reply.kind === 'error') return {kind:'refused',reason:{code:'provider_refused',message:reply.error.message},beforeAcceptance:true};
    if (op.kind === 'forkThread') {
      if (state.stage === 'threadHistoryTurns') {
        const found = Array.isArray(reply.value.data) && reply.value.data.some(turn=>turn.id === op.turnId && turn.status !== 'inProgress');
        if (!found) return {kind:'refused',reason:{code:'provider_refused',message:'selected turn unavailable'},beforeAcceptance:true};
        return {kind:'continue',plan:{action:{kind:'request',call:{method:'thread/fork',params:{threadId:op.sourceThreadId,lastTurnId:op.turnId}}},state:{stage:'thread/fork',sourceThreadId:op.sourceThreadId,turnId:op.turnId}},beforeAcceptance:false};
      }
      if (state.stage !== 'thread/fork') return {kind:'refused',reason:{code:'provider_refused',message:'Fork did not reach its provider boundary'},beforeAcceptance:false};
      const threadId=reply.value.thread && reply.value.thread.id;
      if (!threadId || threadId === op.sourceThreadId || state.sourceThreadId !== op.sourceThreadId || state.turnId !== op.turnId)
        return {kind:'refused',reason:{code:'provider_refused',message:'Forked thread identity unverified'},beforeAcceptance:false};
      return {kind:'complete',result:{kind:'threadForked',sourceThreadId:op.sourceThreadId,threadId,turnId:op.turnId}};
    }
    switch (op.kind) {
      case 'backgroundProcesses': return {kind:'complete',result:op.action.kind === 'list' ? {kind:'backgroundProcesses',threadId:op.threadId,processes:reply.value.processes,nextCursor:reply.value.nextCursor || null} : op.action.kind === 'terminate' ? {kind:'backgroundProcessTermination',threadId:op.threadId,processId:op.action.processId,acknowledged:reply.value.acknowledged === true} : {kind:'done'}};
      case 'initialize': return {kind:'complete',result:{kind:'initialized'}};
      case 'openThread': return {kind:'complete',result:{kind:'threadLoaded',threadId:reply.value.threadId,settings:op.settings}};
      case 'resumeThread': return {kind:'complete',result:{kind:'threadLoaded',threadId:op.threadId,settings:op.settings}};
      case 'readHistory': if (reply.value.more) return {kind:'continue',plan:{action:{kind:"request",call:{method:op.kind,params:op}},state:null},history:{kind:'historyPage',threadId:op.threadId,items:reply.value.items,openItems:[]},beforeAcceptance:false}; return {kind:'complete',result:{kind:'historyPage',threadId:op.threadId,items:reply.value.items,openItems:[]}};
      case 'submitTurn': return {kind:'complete',result:{kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId}};
      case 'control': if (op.action === 'goal') return {kind:'complete',result:{kind:'goal',threadId:op.threadId,intent:reply.value.intent}}; if (op.action === 'redeemResetCredit') return {kind:'complete',result:{kind:'resetCreditConsumed',outcome:reply.value.outcome}}; return {kind:'complete',result:{kind:'done'}};
      default: return {kind:'complete',result:{kind:'done'}};
    }
  },
  runtimeReduce(ctx, state, frames) {
    const events = frames.map(frame => frame.params.usageEvent || ({kind:'chatItem',threadId:ctx.threadId,item:frame.params}));
    return {state:{count:(state ? state.count : 0)+frames.length},events};
  }
};
