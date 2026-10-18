import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {requireValue} from './model.mjs';

export const readJson = file => JSON.parse(fs.readFileSync(file,'utf8'));
export function atomic(file,value) {
  fs.writeFileSync(file+'.tmp',JSON.stringify(value,null,2)+'\n',{mode:0o600}); fs.renameSync(file+'.tmp',file);
}
export function waitFile(file,signal) {
  return new Promise((resolve,reject) => {
    const finish = (error,value) => { watcher.close(); signal.removeEventListener('abort',abort); error?reject(error):resolve(value); };
    const check = () => { try { if (fs.existsSync(file)) finish(null,readJson(file)); } catch (error) { finish(error); } };
    const abort = () => finish(Error('File observation deadline'));
    const watcher = fs.watch(path.dirname(file),check);
    signal.addEventListener('abort',abort,{once:true});
    if (signal.aborted) abort(); else check();
  });
}
export function request(endpoint,message,signal) {
  return new Promise((resolve,reject) => {
    const socket = net.createConnection(endpoint); let data = '';
    const abort = () => socket.destroy(Error('Emitter acknowledgement deadline'));
    socket.on('connect',() => socket.write(JSON.stringify(message)+'\n'));
    socket.on('data',chunk => {
      data += chunk; if (data.length > 8192) return socket.destroy(Error('Oversized emitter reply'));
      if (data.includes('\n')) {
        try { const reply = JSON.parse(data.split('\n')[0]); requireValue(reply.ok,reply.error??'Emitter refused'); resolve(reply); socket.end(); }
        catch (error) { socket.destroy(error); }
      }
    });
    socket.on('error',reject);
    socket.on('close',() => { signal.removeEventListener('abort',abort); reject(Error('Emitter closed without acknowledgement')); });
    signal.addEventListener('abort',abort,{once:true}); if (signal.aborted) abort();
  });
}
export function processInfo(pid) {
  requireValue(Number.isSafeInteger(pid) && pid > 0,'Invalid process ID');
  if (process.platform === 'win32') {
    const code = `$p=Get-Process -Id ${pid} -ErrorAction Stop; [pscustomobject]@{pid=$p.Id;created=$p.StartTime.ToUniversalTime().ToString('o');createdFiletime=$p.StartTime.ToUniversalTime().ToFileTimeUtc().ToString();executable=$p.Path;privateBytes=$p.PrivateMemorySize64;workingSet=$p.WorkingSet64;cpuSeconds=$p.TotalProcessorTime.TotalSeconds}|ConvertTo-Json -Compress`;
    return JSON.parse(execFileSync('powershell.exe',['-NoLogo','-NoProfile','-Command',code],{encoding:'utf8',timeout:15_000}));
  }
  const helper = fileURLToPath(new URL('../../restart-endurance/',import.meta.url));
  const code = "import sys,platform;sys.path.insert(0,sys.argv[1]);import listener;print((listener.mac_start if platform.system()=='Darwin' else listener.linux_start)(int(sys.argv[2])))";
  const created = execFileSync(process.env.PYTHON??'python3',['-c',code,helper,String(pid)],{encoding:'utf8',timeout:5000}).trim();
  requireValue(/^\d+$/.test(created),'Kernel process creation identity missing');
  const rss = execFileSync('ps',['-p',String(pid),'-o','rss='],{encoding:'utf8',timeout:5000}).trim();
  const workingSet = rss.length?Number(rss)*1024:null;
  return {pid,created,privateBytes:null,cpuSeconds:null,workingSet:workingSet !== null && Number.isFinite(workingSet)?workingSet:null};
}
export function shellCommand(argv) {
  if (process.platform === 'win32') {
    const code = '& '+argv.map(arg => "'"+arg.replaceAll("'","''")+"'").join(' ');
    return 'powershell.exe -NoLogo -NoProfile -EncodedCommand '+Buffer.from(code,'utf16le').toString('base64');
  }
  return argv.map(arg => "'"+arg.replaceAll("'","'\\''")+"'").join(' ');
}
