(context => {
  if(!/^[A-Za-z0-9_-]+$/.test(context.tabId))throw Error('Owned tab required');
  const key='__curatedQaResetObserver';
  if(context.phase==='install'){
    if(window[key])throw Error('Recorder already installed');
    const recorder={original:window.fetch,requests:[],tabId:context.tabId};
    window[key]=recorder;
    window.fetch=function(input,init){const url=typeof input==='string'?input:input.url;if(url.includes(`/api/tabs/${context.tabId}/agent/runtime/reset-credits`)){let body;try{body=JSON.parse(init?.body||'{}')}catch{}recorder.requests.push({action:body?.action??'unparsed',confirmed:body?.confirmed===true})}return recorder.original.apply(this,arguments)};
    return {installed:true};
  }
  const recorder=window[key];if(!recorder||recorder.tabId!==context.tabId)throw Error('Owned recorder missing');
  const dialogs=[...document.querySelectorAll('[role="dialog"]')];
  const result={requests:recorder.requests,consume_requests:recorder.requests.filter(x=>x.action==='redeem').length,dialog_text:dialogs.find(e=>e.getAttribute('aria-label')==='usage details')?.innerText??null,confirmation_open:dialogs.some(e=>e.getAttribute('aria-label')==='Reset current rate limits?')};
  if(context.phase==='restore'){window.fetch=recorder.original;delete window[key]}
  return result;
})
