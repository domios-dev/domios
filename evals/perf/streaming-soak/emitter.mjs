import net from 'node:net';
import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {atomic,readJson} from './io.mjs';
import {emitterDecision,requireValue,VERSION} from './model.mjs';

const [mode,configFile] = process.argv.slice(2);
const config = readJson(configFile);
requireValue(config.version === VERSION && config.fixtureOnly === true,'Fixture-only config required');
if (mode === 'supervise') {
  const child = spawn(process.execPath,[fileURLToPath(import.meta.url),'serve',configFile],{stdio:['ignore','inherit','inherit']});
  child.on('error',error => { atomic(config.exit,{run:config.run,error:String(error),code:null}); process.exitCode = 1; });
  child.on('exit',(code,signal) => {
    atomic(config.exit,{run:config.run,pid:child.pid,code,signal,observed:'child exit event'});
    process.exitCode = code??1;
  });
} else if (mode === 'serve') {
  const state = {run:config.run,token:config.token,batch:0,stopped:false};
  let busy = false;
  const server = net.createServer(socket => {
    let data = '', handled = false;
    socket.on('error',() => {});
    socket.on('data',async chunk => {
      data += chunk;
      if (data.length > 4096) return socket.destroy();
      if (handled || !data.includes('\n')) return;
      handled = true;
      let acquired = false;
      const reply = value => socket.end(JSON.stringify({version:VERSION,run:state.run,ok:true,...value})+'\n');
      try {
        requireValue(!busy,'Emitter already has an in-flight request');
        const decision = emitterDecision(state,JSON.parse(data.split('\n')[0]));
        busy = true; acquired = true;
        if (decision.op === 'batch') {
          const output = Array.from({length:decision.lines},(_,i) =>
            `\x1b[36mSOAK ${state.run.slice(0,8)} ${decision.batch}:${i}\x1b[0m ${'bounded-output '.repeat(8)}\r\n`).join('');
          await new Promise((resolve,reject) => process.stdout.write(output,error => error?reject(error):resolve()));
          state.batch = decision.batch;
        }
        if (decision.op === 'stop') state.stopped = true;
        reply({batch:state.batch,pid:process.pid});
        busy = false;
        if (state.stopped) server.close();
      } catch (error) {
        if (acquired) busy = false;
        socket.end(JSON.stringify({ok:false,error:String(error)})+'\n');
      }
    });
  });
  server.on('error',error => { process.stderr.write(String(error)+'\n'); process.exitCode = 1; });
  server.listen(config.endpoint,() => atomic(config.ready,{run:state.run,pid:process.pid,endpoint:config.endpoint}));
} else throw Error('Expected supervise or serve');
