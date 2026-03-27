/**
 * Quick parser test — run with: npx tsx tests/parser-test.ts [file]
 * Reads a Claude output dump and shows parsed messages with type labels.
 */
import { readFileSync, readdirSync } from 'fs';
import { parseClaudeOutput } from '../src/components/ChatView/parser';

const file = process.argv[2]
  || readdirSync('/tmp/codeterm-parser-tests').filter(f => f.endsWith('.txt') && f !== 'claude_56903565.txt')[0]
    ? `/tmp/codeterm-parser-tests/${readdirSync('/tmp/codeterm-parser-tests').filter(f => f.endsWith('.txt') && !f.includes('claude_56903565'))[0]}`
    : null;

if (!file) {
  console.error('Usage: npx tsx tests/parser-test.ts <file>');
  console.error('Or dump pane outputs first via the dev API');
  process.exit(1);
}

console.log(`Parsing: ${file}\n`);
const text = readFileSync(file, 'utf-8');
const messages = parseClaudeOutput(text);

console.log(`Found ${messages.length} messages:\n`);

const COLORS = {
  user: '\x1b[36m',      // cyan
  assistant: '\x1b[32m', // green
  tool_call: '\x1b[33m', // yellow
  tool_result: '\x1b[90m', // gray
  status: '\x1b[35m',    // magenta
};
const RESET = '\x1b[0m';

for (const msg of messages) {
  const color = COLORS[msg.type] || '';
  const label = msg.type.toUpperCase().padEnd(12);
  const preview = msg.content.replace(/\n/g, '\\n').slice(0, 120);
  const tool = msg.toolName ? ` [${msg.toolName}(${msg.toolArgs?.slice(0, 40) || ''})]` : '';
  const dur = msg.duration ? ` (${msg.duration})` : '';
  console.log(`${color}${label}${RESET}${tool}${dur} ${preview}`);
}

console.log(`\n--- Summary ---`);
const counts: Record<string, number> = {};
for (const m of messages) {
  counts[m.type] = (counts[m.type] || 0) + 1;
}
for (const [type, count] of Object.entries(counts).sort((a, b) => b[1] - a[1])) {
  console.log(`  ${type}: ${count}`);
}
