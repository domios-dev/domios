import json,sys,os
tracked={}
for l in open(sys.argv[1]):
    if not l.strip(): continue
    pid,prov,ws,cwd=l.split(None,3)
    tracked[pid]=(prov,cwd.strip())
tabs={p['id']:p for p in json.load(open(sys.argv[2]))}

live={pid:tabs[pid] for pid in tracked if pid in tabs}
unbound=[pid for pid in live if not live[pid].get('agent_session_id')]

# uniqueness: sid -> [tabs]
from collections import defaultdict
bysid=defaultdict(list)
for pid,p in live.items():
    sid=p.get('agent_session_id')
    if sid: bysid[sid].append(pid)
dupes={sid:ps for sid,ps in bysid.items() if len(ps)>1}

print(f"  tracked={len(tracked)} live={len(live)} bound={len(live)-len(unbound)} unbound={len(unbound)}")
rc=0
if not tracked or len(live) != len(tracked):
    print("  EMPTY OR MISSING fleet: every tracked tab must be live"); rc=1
if unbound:
    print(f"  UNBOUND tabs: {unbound[:8]}"); rc=1
if dupes:
    for sid,ps in list(dupes.items())[:8]:
        print(f"  DUPLICATE sid {sid[:12]} claimed by {ps}")
    rc=1
sys.exit(rc)
