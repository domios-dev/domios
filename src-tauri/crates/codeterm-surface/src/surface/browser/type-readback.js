function domiosTypeReadback(secret) {
 if (!this.isConnected) return {ok:false, reason:'detached'};
 if (secret) {
  if (!(this instanceof HTMLInputElement)) return {ok:false, reason:'unmaskable'};
  return {ok:true, length:this.value.length};
 }
 if (this instanceof HTMLInputElement || this instanceof HTMLTextAreaElement) return {ok:true, text:this.value};
 if (this.isContentEditable) return {ok:true, text:typeof this.innerText === 'string' ? this.innerText : (this.textContent || '')};
 return {ok:false, reason:'unsupported_input'};
}
