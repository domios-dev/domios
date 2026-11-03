(selector, grep, limit) => {
 const root = selector ? document.querySelector(selector) : document.body;
 if (!root) throw new Error('not_found: text selector has no match');
 let text = root.innerText || '';
 if (text.length > 2000000) throw new Error('text_limit: narrow the selector');
 const sensitive = '[data-sensitive], input[type="password"], input[autocomplete="current-password"], input[autocomplete="new-password"]';
 if (root.matches(sensitive) || root.closest('[data-sensitive]')) return 'REDACTED';
 for (const node of root.querySelectorAll(sensitive)) {
  const value = node.innerText;
  if (value) text = text.split(value).join('REDACTED');
 }
 if (grep !== null) text = text.split(/\r?\n/).filter(line => line.toLowerCase().includes(grep.toLowerCase())).join('\n');
 return text.length > limit ? text.slice(0, limit) + '\n… (truncated; narrow --selector/--grep or raise the limit)' : text || (grep === null ? '(empty page)' : '(no matching lines)');
}
