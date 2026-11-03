function domiosVisible() {
 if (!this.isConnected || this.closest('[hidden], [inert]')) return false;
 const style = getComputedStyle(this);
 return style.visibility !== 'hidden' && style.visibility !== 'collapse' && style.display !== 'none' && this.getClientRects().length > 0;
}
