import "./core-concepts.css";
import type { ReactNode } from "react";

interface ConceptProps {
  number: string;
  name: string;
  note: string;
  children: ReactNode;
}

function Concept({ number, name, note, children }: ConceptProps) {
  return (
    <article className="concept-card">
      <header>
        <span>{number}</span>
        <div><h3>{name}</h3><p>{note}</p></div>
      </header>
      <div className="concept-stage" role="img" aria-label={`${name}: animated team working indicator`}>
        <div className="mini-team"><b>Team</b><span>2</span></div>
        <div className="concept-motion">{children}</div>
      </div>
    </article>
  );
}

export function CoreConcepts() {
  return (
    <section className="concept-section" aria-labelledby="core-concepts-title">
      <div className="section-heading">
        <div><span className="section-index">01</span><h2 id="core-concepts-title">Forge signals</h2></div>
        <p>Built by Codex · compact mechanical metaphors</p>
      </div>
      <div className="concept-grid">
        <Concept number="01" name="Micro forge" note="A soft hammer strike with restrained sparks.">
          <span className="forge-anvil" />
          <span className="forge-hammer"><i /></span>
          <span className="forge-spark spark-a" /><span className="forge-spark spark-b" /><span className="forge-spark spark-c" />
        </Concept>
        <Concept number="02" name="Workbeat" note="Three beats travel through a quiet status rail.">
          <span className="workbeat-rail"><i /><i /><i /></span>
        </Concept>
        <Concept number="03" name="Relay orbit" note="A task handoff moving between two teammates.">
          <span className="relay-node relay-left" /><span className="relay-node relay-right" /><span className="relay-path" /><span className="relay-packet" />
        </Concept>
        <Concept number="04" name="Blueprint scan" note="A narrow scan reveals work under construction.">
          <span className="blueprint"><i /><i /><i /></span><span className="blueprint-scan" />
        </Concept>
        <Concept number="05" name="Pixel assembly" note="Fragments settle into a finished unit, then reset.">
          <span className="pixel-grid">{Array.from({ length: 6 }, (_, index) => <i key={index} />)}</span>
        </Concept>
        <Concept number="06" name="Tool carousel" note="Two tools exchange focus without visual noise.">
          <span className="tool-ring"><i className="tool-code">‹›</i><i className="tool-build">◆</i></span>
        </Concept>
        <Concept number="07" name="Signal braid" note="Independent workstreams briefly synchronize.">
          <span className="signal-braid"><i /><i /><i /></span>
        </Concept>
        <Concept number="08" name="Commit press" note="A tiny plate stamps completed work into the rail.">
          <span className="commit-press"><i /></span><span className="commit-plate" /><span className="commit-line" />
        </Concept>
      </div>
    </section>
  );
}
