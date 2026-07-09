import { RichMarkdown } from "@codeterm/chat-ui";
import type { Plan } from "../types";
import { latestRetrospective, showRetrospectiveBlock } from "./planRetrospective";

export interface PlanRetrospectiveBlockProps {
  plan: Plan;
}

export function PlanRetrospectiveBlock({ plan }: PlanRetrospectiveBlockProps) {
  if (!showRetrospectiveBlock(plan)) return null;
  const retrospective = latestRetrospective(plan);
  return (
    <section data-testid="plan-retrospective-block" className="plan-retrospective-block">
      <div className="plan-retrospective-label">Retrospective</div>
      {retrospective ? (
        <div data-testid="plan-retrospective-body" className="plan-retrospective-body">
          <RichMarkdown text={retrospective.text} />
        </div>
      ) : (
        <p data-testid="plan-retrospective-missing" className="plan-retrospective-missing">
          No retrospective recorded yet. Write one with{" "}
          <code>{`codeterm plan retrospective ${plan.id} --body "..."`}</code>.
        </p>
      )}
    </section>
  );
}
