module.exports = {
  runtimeStoredHistory(session, records) {
    const value = JSON.parse(records);
    if (value.session !== session) throw new Error('foreign session');
    return value.messages;
  },
  runtimePlan(_ctx, op) {
    if (op.kind === 'answerQuestion') return {action:{kind:'questionAnswer',input:{text:op.answers.flatMap(answer=>answer.values).join('\n'),attachments:[]}},state:null};
    if (op.kind === 'answer') return {action:{kind:'reply',requestId:op.requestId,result:{kind:'success',value:op.answer}},state:null};
    if (op.kind === 'interrupt') return {action:{kind:'request',call:{method:op.kind,params:op,target:{threadId:op.threadId,...(op.activeTurnId ? {turnId:op.activeTurnId} : {})}}},state:null};
    return {action:{kind:'request',call:{method:op.kind,params:op}},state:null};
  },
  runtimeResponse(_ctx, op, _state, reply) {
    if (reply.kind === 'error') return {kind:'refused',reason:{code:'provider_refused',message:reply.error.message},beforeAcceptance:true};
    switch (op.kind) {
      case 'initialize': return {kind:'complete',result:{kind:'initialized'}};
      case 'openThread': return {kind:'complete',result:{kind:'threadLoaded',threadId:reply.value.threadId,settings:op.settings}};
      case 'resumeThread': return {kind:'complete',result:{kind:'threadLoaded',threadId:op.threadId,settings:op.settings}};
      case 'readHistory': if (reply.value.more) return {kind:'continue',plan:{action:{kind:"request",call:{method:op.kind,params:op}},state:null},history:{kind:'historyPage',threadId:op.threadId,items:reply.value.items,openItems:[]},beforeAcceptance:false}; return {kind:'complete',result:{kind:'historyPage',threadId:op.threadId,items:reply.value.items,openItems:[]}};
      case 'submitTurn': return {kind:'complete',result:{kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId}};
      default: return {kind:'complete',result:{kind:'done'}};
    }
  },
  runtimeReduce(ctx, state, frames) {
    const events = frames.map(frame => frame.params.usageEvent || ({kind:'chatItem',threadId:ctx.threadId,item:frame.params}));
    return {state:{count:(state ? state.count : 0)+frames.length},events};
  }
};
