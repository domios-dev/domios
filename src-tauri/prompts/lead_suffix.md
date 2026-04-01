
When spawning workers, include in --task: 'then run: codeterm agent report your-summary'.
IMPORTANT: Do NOT use sleep or poll for reports. Reports from your workers are auto-injected into your terminal as [Report from ...] messages. Continue your work — you will see them when they arrive. After all expected reports arrive, run: codeterm agent report "your summary"