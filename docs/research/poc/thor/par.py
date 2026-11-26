import json, time, urllib.request, concurrent.futures as cf, sys
port = int(sys.argv[1])
st = "Agent A is at column 3, row 4. The beacon B is at column 9, row 2. A walking path to the beacon exists and is 8 steps long. Enemy 1 is 5 cells away and CAN see agent A (clear line of sight)."
Q = {"action": {"type": "choice", "instructions": "Agent A's best action: hide if an enemy can see A within 6 cells; else advance if the beacon is reachable within 10 steps; else scout.",
       "criteria": {"hide": "an enemy can see A within 6 cells", "advance": "no visible enemy and beacon reachable in <=10 steps", "scout": "neither"}},
     "enemy_visible": {"type": "noul", "instructions": "Some enemy can see agent A within 6 cells."}}
def call(_):
    b = json.dumps({"model": "m", "state": st, "questions": Q}).encode()
    r = urllib.request.Request(f"http://192.168.0.78:{port}/v1/systemone", data=b, headers={"Content-Type": "application/json"})
    t = time.perf_counter(); urllib.request.urlopen(r, timeout=30).read(); return (time.perf_counter() - t) * 1000
for _ in range(3): call(0)
t = time.perf_counter(); seq = [call(0) for _ in range(8)]; ts = (time.perf_counter() - t) * 1000
with cf.ThreadPoolExecutor(8) as ex:
    t = time.perf_counter(); par = list(ex.map(call, range(8))); tp = (time.perf_counter() - t) * 1000
print(f"port {port}: 8 sequential = {ts:.0f} ms total ({ts/8:.0f}/call) | 8 concurrent = {tp:.0f} ms wall, per-call {min(par):.0f}-{max(par):.0f} ms")
