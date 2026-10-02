import './jev.css';

function JevReflex() {
  return <figure className="jev-reflex" aria-labelledby="jev-reflex-caption">
    <figcaption id="jev-reflex-caption" className="jev-diagram-caption"><span>SMALL MODEL. QUICK REFLEXES.</span><span>Illustrative flow</span></figcaption>
    <div className="jev-inputs"><span><i />Tab activity</span><span><i />Memory</span><span><i />Host events</span></div>
    <div className="jev-core-stage">
      <div className="jev-streams" aria-hidden="true"><i /><i /><i /></div>
      <div className="jev-orbit" aria-hidden="true" />
      <div className="jev-core"><div className="jev-liquid" aria-hidden="true" /><span>JEV<small>decision model</small></span></div>
      <span className="jev-signal jev-signal-left" aria-hidden="true">observe</span><span className="jev-signal jev-signal-right" aria-hidden="true">understand</span>
    </div>
    <div className="jev-verdict"><span className="jev-verdict-dot" aria-hidden="true" /><span>Blocked on a prompt</span><span className="jev-verdict-label">STATE</span></div>
    <div className="jev-evidence">
      <span className="jev-gate-icon" aria-hidden="true">↳</span>
      <div><span className="jev-evidence-label">EVIDENCE BEFORE ACTION</span><p>Permission required? <strong>Your call.</strong></p><p>Provider exited? <strong>Bounded recovery.</strong></p></div>
    </div>
    <p className="jev-boundary">JEV never grants permissions or types answers for you.</p>
  </figure>;
}

export function JevSection() {
  return <section id="jev" className="jev-section section-wrap" aria-labelledby="jev-title">
    <div className="jev-heading"><p className="eyebrow">MEET THE SPINAL CORD</p><h2 id="jev-title">The OS thinks<br /><span>for itself.</span></h2><p>A little intelligence in the infrastructure. JEV is the small, fast decision model that helps Domios keep its bearings.</p></div>
    <div className="jev-layout">
      <JevReflex />
      <div className="jev-details">
        <ol className="jev-points">
          <li><span className="jev-number" aria-hidden="true">01</span><div><h3>A pulse on every tab.</h3><p>Working, idle, blocked on a prompt, or crashed. JEV gives what’s happening a name.</p></div></li>
          <li><span className="jev-number" aria-hidden="true">02</span><div><h3>The right things remembered.</h3><p>Ranks what your agents should remember, so useful context comes first.</p></div></li>
          <li><span className="jev-number" aria-hidden="true">03</span><div><h3>Reflexes for when you’re away.</h3><p>Helps Domios recover the General agent within hard limits and coordinate provider updates.</p></div></li>
        </ol>
        <div className="jev-connect" aria-label="Example conversation to connect JEV"><p><span>you</span><code>connect JEV</code></p><p><span>agent</span><span>Done. JEV is watching your tabs.</span></p></div>
        <p className="jev-setup">Just ask your agent. Domios handles the rest.<br />Prefer a switch? Connect it in Settings.</p>
      </div>
    </div>
    <div className="jev-footer"><p className="jev-principle">The model names the state.<br /><strong>Deterministic evidence authorizes the action.</strong></p><p>One connection. Every reflex. No feature switches.<br />Local models too, through adapters such as LM Studio. Spend stays visible.<br />Without JEV, Domios works exactly as before.</p></div>
  </section>;
}
