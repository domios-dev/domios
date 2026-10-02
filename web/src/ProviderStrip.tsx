import { Icon } from './Icon';
import type { IconName } from './Icon';

const providers: { name: string; icon: IconName }[] = [
  { name: 'Claude Code', icon: 'branch' },
  { name: 'Codex', icon: 'terminal' },
  { name: 'OpenCode', icon: 'grid' },
  { name: 'Cursor', icon: 'arrow' },
  { name: 'Grok Build', icon: 'mesh' },
  { name: 'LM Studio', icon: 'layers' },
  { name: 'Your agent', icon: 'plus' },
];

export function ProviderStrip() {
  return <section className="provider-strip section-wrap" aria-labelledby="providers-title">
    <h2 id="providers-title">BRING YOUR AGENTS</h2>
    <ul className="provider-list">{providers.map(({ name, icon }) => <li key={name}><Icon name={icon} size={20} /><span>{name}</span></li>)}</ul>
    <p>Missing yours? Ask your agent to write a provider plugin, then share it with everyone.</p>
  </section>;
}
