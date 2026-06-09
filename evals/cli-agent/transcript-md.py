import sys, json

data = json.load(open(sys.argv[1]))
case, provider, prompt = sys.argv[2], sys.argv[3], sys.argv[4]
print(f"# {case} — {provider}\n\n**Prompt:** {prompt}\n")
msgs = data.get("messages", [])
if not msgs:
    print(f"_(no messages — count={data.get('count', 0)}, status={data.get('status','')})_")
for m in msgs:
    t = m.get("type", "?")
    if t == "tool_call":
        cmd = (m.get("toolInput") or {}).get("command", "")
        print(f"- 🔧 **{m.get('toolName','tool')}** `{cmd}`")
    elif t == "tool_result":
        print(f"  ↳ result: {(m.get('toolResult') or '')[:300]}")
    elif t == "thinking":
        print(f"- 💭 _{m.get('content','')[:200]}_")
    else:
        print(f"- **{t}:** {m.get('content','')[:500]}")
