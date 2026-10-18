export async function installReplayPage({tab,session,nonce,run,other,muxTab,probe='composer',maxSamples=20000},loadModule=specifier => import(specifier)) {
  if (window.__streamingSoakReplay) throw Error('Replay instrumentation already installed');
  const bridge = await loadModule('/src/components/Terminal/chatTestBridge.ts');
  if (!bridge.isCodeTermFrontendTestMode()) throw Error('Parent must enable frontend test mode on DEV before setup');
  const store = await loadModule('/src/components/Terminal/transcriptStore.ts');
  const core = await loadModule('/packages/chat-core/src/transcript.ts');
  const {chatProjectionMonitor:monitor} = await loadModule('/src/components/Terminal/chatProjectionDiagnostics.ts');
  const root = document.querySelector(`[data-testid="tab-session-chat"][data-tab-id="${tab}"]`);
  const textarea = root?.querySelector('textarea[aria-label="Message composer"]');
  const graph = document.querySelector('[role="toolbar"][aria-label="Mesh graph controls"]')?.parentElement;
  if (!root || probe === 'composer' && (!textarea || textarea.value !== '')) throw Error('Owned composer mount required');
  if (probe === 'mesh' && (!graph || graph.getBoundingClientRect().width <= 0)) throw Error('Parent must open the visible Mesh machines graph before replay');
  const key = store.transcriptWindowKey(tab,session);
  const read = (target={session,nonce}) => {
    const transcript = store.readTranscriptWindow(target === other?store.transcriptWindowKey(other.tab,other.session):key).transcript;
    if (transcript.authority?.sourceSessionId !== target.session || transcript.authority.sessionBindingNonce !== target.nonce) throw Error('Owned replay authority drift');
    return {authority:transcript.authority,rows:core.transcriptRows(transcript),evictedOlder:transcript.evictedOlder};
  };
  read();
  let active = false, coordinate = null, values, overflow = false, expectedDraft = '', armed = false, timeline = [], mutationCounts, frameEpoch = 0;
  const observers = [], pending = new Set();
  const lifetime = AbortSignal.timeout(450000);
  const expire = () => window.__streamingSoakReplay?.dispose();
  lifetime.addEventListener('abort',expire,{once:true});
  const add = (name,value) => {
    if (!active || !Number.isFinite(value)) return;
    if (values[name].length >= maxSamples) { overflow = true; return; }
    values[name].push(value);
  };
  const record = value => { if (timeline.length >= maxSamples) overflow = true; else timeline.push(value); };
  const frames = () => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  const paintEvents = new EventTarget(), originalPaint = monitor.recordPaint;
  function recordPaint(id,tail) {
    originalPaint.call(this,id,tail);
    if (active && (id === tab || id === other.tab)) record({kind:'tail-paint-opportunity',at:performance.now(),tail,target:id === tab?'local':'other',...coordinate});
    if (id === tab) paintEvents.dispatchEvent(new CustomEvent('tail',{detail:tail}));
  }
  monitor.recordPaint = recordPaint;
  function painted(tail) {
    if (monitor.snapshot(tab)?.paintedTail === tail) return Promise.resolve();
    return new Promise((resolve,reject) => {
      const deadline = AbortSignal.timeout(20000);
      const cleanup = () => { paintEvents.removeEventListener('tail',onTail); deadline.removeEventListener('abort',fail); pending.delete(fail); };
      const fail = () => { cleanup(); reject(Error('Owned tail paint-opportunity deadline')); };
      const onTail = event => { if (event.detail === tail) { cleanup(); resolve(); } };
      pending.add(fail); paintEvents.addEventListener('tail',onTail); deadline.addEventListener('abort',fail,{once:true});
    });
  }
  function dispatch(wire,target={tab,session,nonce}) {
    read(target);
    const start = performance.now(), messages = JSON.parse(wire), parsed = performance.now();
    if (!messages.every(row => row.id.startsWith('soak-'+run+'-') && row.sourceSessionId === target.session)) throw Error('Foreign replay row');
    const tail = messages.at(-1), done = probe === 'composer' && target.tab === tab?painted(tail.id+'#'+tail.content.length):frames();
    window.dispatchEvent(new CustomEvent('codeterm:test-chat-reply',{detail:{tabId:target.tab,messages,sourceSessionId:target.session,sessionBindingNonce:target.nonce}}));
    const returnedAt = performance.now();
    add('parse',parsed-start); add('dispatchToStoreReturn',returnedAt-parsed);
    const committed = read(target).rows.find(row => row.id === tail.id);
    if (committed?.content !== tail.content) throw Error('Owned test listener did not adopt exact tail; never pretend unmounted ingestion succeeded');
    if (active) record({kind:'normalized-arrival-store-return',at:start,returnedAt,...coordinate,
      id:tail.id,seq:tail.seq??null,characters:tail.content.length,bytes:new TextEncoder().encode(tail.content).length,target:target.tab === tab?'local':'other'});
    return done;
  }
  function matches(event) { return probe === 'composer'?event.target === textarea:graph.contains(event.target); }
  const capture = event => {
    if (!active || !matches(event)) return;
    if (!armed || !event.isTrusted) { overflow = true; return; }
    armed = false;
    const start = performance.now();
    event.__soakStart = start;
    record({kind:'input-capture',at:start,...coordinate,trusted:event.isTrusted});
    add('eventTimestampToCapture',event.timeStamp >= 0 && event.timeStamp <= start?start-event.timeStamp:NaN);
    requestAnimationFrame(() => {
      add('inputToFirstFrame',performance.now()-start);
      record({kind:'first-frame-opportunity',at:performance.now(),...coordinate});
      requestAnimationFrame(() => {
        add('inputToSecondFrame',performance.now()-start);
        record({kind:'second-frame-opportunity',at:performance.now(),...coordinate});
        if (probe === 'composer' && (textarea.value !== expectedDraft || !textarea.isConnected || document.activeElement !== textarea)) overflow = true;
      });
    });
  };
  const bubble = event => { if (matches(event) && event.__soakStart != null) add('captureToDocumentBubble',performance.now()-event.__soakStart); };
  const inputEvent = probe === 'composer'?'input':'wheel';
  document.addEventListener(inputEvent,capture,true); document.addEventListener(inputEvent,bubble);
  const graphObserver = graph?new MutationObserver(records => {
    if (active) {
      for (const item of records) mutationCounts[item.type]++;
      record({kind:'graph-dom-mutations',at:performance.now(),records:records.length,...coordinate});
    }
  }):null;
  graphObserver?.observe(graph,{subtree:true,attributes:true,childList:true,characterData:true});
  const supported = globalThis.PerformanceObserver?.supportedEntryTypes??[];
  function observe(type,callback,options={}) {
    if (!supported.includes(type)) return false;
    let observer;
    try {
      observer = new PerformanceObserver(list => callback(list.getEntries()));
      observer.observe({type,buffered:false,...options}); observers.push({observer,callback}); return true;
    } catch { observer?.disconnect(); return false; }
  }
  const longTasks = [];
  const hasLongTask = observe('longtask',entries => {
    for (const entry of entries) if (active) {
      add('longTask',entry.duration);
      longTasks.push({startTime:entry.startTime,duration:entry.duration,callbackCoordinate:coordinate}); longTasks.sort((a,b) => b.duration-a.duration); longTasks.length = Math.min(20,longTasks.length);
    }
  });
  const hasEventTiming = observe('event',entries => {
    for (const entry of entries) if (active && entry.target && (probe === 'composer'?entry.target === textarea:graph.contains(entry.target))) {
      add('eventTimingQueue',entry.processingStart-entry.startTime);
      add('eventTimingHandler',entry.processingEnd-entry.processingStart);
      add('eventTimingDuration',entry.duration);
    }
  },{durationThreshold:16});
  window.__streamingSoakReplay = {
    async seed(wire,targetOther=false) {
      if (active) throw Error('Cannot seed during measured segment');
      if (probe === 'composer' && !targetOther) {
        const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set;
        setter.call(textarea,''); textarea.dispatchEvent(new InputEvent('input',{bubbles:true,inputType:'deleteContentBackward'}));
        expectedDraft = ''; textarea.focus(); textarea.setSelectionRange(0,0);
      }
      await Promise.all([dispatch(wire,targetOther?other:{tab,session,nonce}),frames()]);
      if (probe === 'composer' && !targetOther && textarea.value !== '') throw Error('Owned draft reset did not commit');
    },
    begin() {
      values = Object.fromEntries(['parse','dispatchToStoreReturn','muxTestDispatch','frameInterval','eventTimestampToCapture','captureToDocumentBubble','inputToFirstFrame','inputToSecondFrame','eventTimingQueue','eventTimingHandler','eventTimingDuration','longTask'].map(name => [name,[]]));
      longTasks.length = 0; timeline = []; mutationCounts = {attributes:0,childList:0,characterData:0}; overflow = false; active = true;
      const epoch = ++frameEpoch; let previous = null;
      const frame = now => {
        if (!active || overflow || frameEpoch !== epoch) return;
        if (previous !== null) add('frameInterval',now-previous);
        previous = now; requestAnimationFrame(frame);
      };
      requestAnimationFrame(frame);
      return {hasLongTask,hasEventTiming};
    },
    arm(where) {
      coordinate = where;
      if (!active || armed || probe === 'composer' && (textarea.value !== expectedDraft || document.activeElement !== textarea)) throw Error('Owned edit backpressure violated');
      expectedDraft += 'x'; armed = true;
      const mark = `soak-step-${where.segment}-${where.step}-${where.messageBytes}`;
      performance.mark(mark); performance.clearMarks(mark);
      record({kind:'step-arm',at:performance.now(),...where});
      if (probe === 'mesh') {
        const box = graph.getBoundingClientRect();
        for (const [u,v] of [[.95,.95],[.05,.95],[.95,.5],[.5,.95]]) {
          const x = box.x+box.width*u, y = box.y+box.height*v, element = document.elementFromPoint(x,y);
          if (element && graph.contains(element) && !element.closest('[data-peer-card],[data-graph-toolbar]')) return {x,y};
        }
        throw Error('No uncovered graph input point');
      }
    },
    async step(wire,control) {
      if (control === 'mux') {
        if (!window.__muxPerfChurnTab) throw Error('Existing frontend mux test hook unavailable');
        const start = performance.now(); window.__muxPerfChurnTab(muxTab);
        add('muxTestDispatch',performance.now()-start);
      }
      await Promise.all([wire === null?frames():dispatch(wire,control === 'other'?other:{tab,session,nonce}),frames()]);
      return true;
    },
    async acknowledge() {
      await frames();
      if (overflow || armed || probe === 'composer' && (textarea.value !== expectedDraft || document.activeElement !== textarea)) throw Error('Input event/draft/focus/sample completeness failed');
      return {draftCharacters:expectedDraft.length};
    },
    end() {
      for (const {observer,callback} of observers) callback(observer.takeRecords());
      for (const item of graphObserver?.takeRecords()??[]) mutationCounts[item.type]++;
      active = false;
      if (overflow) throw Error('Replay sample budget or edit identity failed');
      return {values,longTasks:[...longTasks],timeline,graphMutations:graph?mutationCounts:null,hasLongTask,hasEventTiming,draftCharacters:probe === 'composer'?expectedDraft.length:null,inputActions:expectedDraft.length};
    },
    read, readOther:() => read(other),
    dispose() {
      active = false; for (const {observer} of observers) observer.disconnect();
      lifetime.removeEventListener('abort',expire);
      graphObserver?.disconnect();
      for (const fail of [...pending]) fail();
      document.removeEventListener(inputEvent,capture,true); document.removeEventListener(inputEvent,bubble);
      if (monitor.recordPaint === recordPaint) monitor.recordPaint = originalPaint;
      delete window.__streamingSoakReplay;
    },
  };
  return {owned:true,boundary:'Normalized test-event dispatch -> existing transcript reducer -> ancestor/render/paint opportunity; no provider/SSE/native IPC adoption',
    timeOrigin:performance.timeOrigin,clock:'performance.now milliseconds; cross-process alignment requires measured clock uncertainty',hasLongTask,hasEventTiming};
}
