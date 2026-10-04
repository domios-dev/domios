import { Icon } from './Icon';
import type { IconName } from './Icon';
import './built-in.css';

const features: { icon: IconName; title: string; text: string }[] = [
  { icon: 'clock', title: 'Scheduled work', text: 'Run agent tasks later or on a schedule.' },
  { icon: 'browser', title: 'Desktop to phone', text: 'Start on your computer. Continue on your phone with any connected agent, over your private Tailnet.' },
  { icon: 'microphone', title: 'Voice messages', text: 'Speak to agents through a transcription plugin.' },
  { icon: 'lock', title: 'Local secrets', text: 'Keep credentials in the local secret store.' },
];

export function BuiltIn() {
  return <section id="foundation" className="built-in section-wrap" aria-labelledby="built-in-title">
    <h2 id="built-in-title">Also built in.</h2>
    <ul className="built-in-list">{features.map(feature => <li key={feature.title}>
      <Icon name={feature.icon} size={24} />
      <div><h3>{feature.title}</h3><p>{feature.text}</p></div>
    </li>)}</ul>
  </section>;
}
