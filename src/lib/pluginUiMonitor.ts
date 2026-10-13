export const PLUGIN_UI_MONITOR = `
(function(){
var failed=false,last='';
function send(status,reason){var key=status+':'+(reason||'');if(key===last)return;last=key;parent.postMessage({__ctUiState:{status:status,reason:reason}},'*');}
function fail(reason){failed=true;send('error',reason);}
window.addEventListener('error',function(e){fail(e.message||'Plugin UI script failed to load.');});
window.addEventListener('unhandledrejection',function(e){fail(String(e.reason||'Plugin UI request failed.'));});
document.addEventListener('securitypolicyviolation',function(e){if(e.effectiveDirective==='script-src-elem'||e.effectiveDirective==='script-src')fail('Plugin UI script was blocked by its security policy.');});
function inspect(){
 if(failed)return;
 var root=document.getElementById('ct-root');
 var content=root||document.body;
 var present=content&&Array.from(content.childNodes).some(function(n){return n.nodeType===3?n.textContent.trim().length>0:n.nodeType===1&&!/^(SCRIPT|STYLE|LINK|META)$/.test(n.tagName);});
 send(present?'ready':'empty',present?undefined:'The plugin rendered no content'+(root?' into #ct-root. Check the plugin mount target.':'.'));
}
function watch(){new MutationObserver(inspect).observe(document.body,{childList:true,subtree:true,characterData:true});inspect();}
document.readyState==='loading'?document.addEventListener('DOMContentLoaded',watch,{once:true}):watch();
})();`;
