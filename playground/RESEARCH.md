# Motion playground research

## Decision

Start with a focused Vite lab inside the repository. It gives CodeTerm one dense comparison canvas, shared animation controls, HMR, and no additional dependency. Keep experiments isolated from production code until a concept wins.

Promote the lab to Ladle or React Cosmos when it becomes a broader component catalog. Choose Storybook when visual regression, interaction tests, and published component documentation become primary requirements.

## Compared approaches

| Approach | Strength | Cost for this phase | Fit |
| --- | --- | --- | --- |
| Focused Vite lab | Total control over side-by-side motion comparison and knobs | We own the small gallery shell | Best now |
| Ladle | Vite-powered stories, controls, themes, and accessibility tooling | New dependency and story conventions | Strong next step |
| React Cosmos | Excellent fixtures, live inputs, responsive and remote renderers | Separate fixture runtime and configuration | Best for state-heavy component exploration |
| Storybook | Mature stories, interaction, visual, accessibility, and CI tooling | Largest setup and conceptual surface | Best once this is a production component workshop |

## Design guidance adopted

- Use a clear aesthetic direction rather than generic decorative motion.
- Treat one strong, legible motion idea as better than many unrelated effects.
- Keep ambient status animation compact and quiet; it must not steal attention from content.
- Animate transforms and opacity for predictable rendering performance.
- Expose theme accent and tempo as shared variables so concepts compare under the same conditions.
- Preserve a meaningful static state and honor `prefers-reduced-motion`.

## Sources

- Anthropic frontend-design skill: <https://github.com/anthropics/claude-code/blob/main/plugins/frontend-design/skills/frontend-design/SKILL.md>
- Motion library and examples: <https://github.com/motiondivision/motion>
- Motion accessibility guidance: <https://motion.dev/docs/react-accessibility>
- MDN reduced-motion reference: <https://developer.mozilla.org/en-US/docs/Web/CSS/Reference/At-rules/%40media/prefers-reduced-motion>
- Storybook interaction testing: <https://storybook.js.org/docs/9/writing-tests/interaction-testing>
- React Cosmos fixtures: <https://reactcosmos.org/docs/fixtures>
- React Cosmos playground UI: <https://reactcosmos.org/docs/user-interface>
- Ladle stories: <https://ladle.dev/docs/stories/>
