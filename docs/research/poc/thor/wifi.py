import json, time, urllib.request, statistics as st
def call(port):
    b = json.dumps({"model":"m","state":"A light is on.","questions":{"on":{"type":"noul","instructions":"The light is on"}}}).encode()
    r = urllib.request.Request(f"http://192.168.0.78:{port}/v1/systemone", data=b, headers={"Content-Type":"application/json"})
    t = time.perf_counter(); urllib.request.urlopen(r, timeout=10).read(); return (time.perf_counter()-t)*1000
for port in (8095,):
    for _ in range(5): call(port)
    xs = sorted(call(port) for _ in range(200))
    print(f"port {port}: p50 {xs[100]:.1f} ms  p90 {xs[180]:.1f}  p95 {xs[190]:.1f}  p99 {xs[198]:.1f}  max {xs[-1]:.1f}")
