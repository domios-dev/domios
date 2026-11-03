(selector, grep, limit) => {
 const root = selector ? document.querySelector(selector) : document.body;
 if (!root) throw new Error('not_found: text selector has no match');
 let text = root.innerText || '';
 if (text.length > 2000000) throw new Error('text_limit: narrow the selector');
 const sensitive = '[data-sensitive], input[type="password"], input[autocomplete="current-password"], input[autocomplete="new-password"]';
 if (root.matches(sensitive) || root.closest('[data-sensitive]')) return {text:'REDACTED',items:1,truncated:false};
 for (const node of root.querySelectorAll(sensitive)) {
  const value = node.innerText;
  if (value) text = text.split(value).join('REDACTED');
 }
 if (grep !== null) text = text.split(/\r?\n/).filter(line => line.toLowerCase().includes(grep.toLowerCase())).join('\n');
 const truncated = text.length > limit;
 const kept = text.slice(0, limit);
 return {text: truncated ? kept + '\n… (truncated; narrow --selector/--grep or raise the limit)' : text || (grep === null ? '(empty page)' : '(no matching lines)'), items: kept ? kept.split(/\r?\n/).length : 0, truncated};
}
