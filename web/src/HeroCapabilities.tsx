import { Icon } from './Icon';
import './hero-capabilities.css';

export function HeroCapabilities() {
  return <div className="hero-details">
    <ul className="hero-capabilities">
      <li><span>One memory. Every agent.</span><small>Shared on this machine.</small></li>
      <li><span>Scheduled work</span><small>Tasks on your schedule.</small></li>
      <li><span>Your messenger</span><small>Telegram updates &amp; replies</small></li>
    </ul>
    <div className="provider-plugin-note">
      <span className="plugin-note-icon"><Icon name="plus" size={20} /></span>
      <p><strong>Build your agent’s plugin with Domios.</strong><span>Ask your agent to write it. Publish from your repository and share it.</span></p>
    </div>
  </div>;
}
