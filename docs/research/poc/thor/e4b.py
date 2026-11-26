"""E4: can Jev read a text-rendered tactical grid? Conditions (syzygy-curriculum method):
FRAME (the grid), BLIND (no grid), SHUFFLED (another scene's grid), GATES (grid + code-computed gate answers).
Ground truth is computed from the grid, never from a model. Key stays in jev.py (local file -> TypeSafe only)."""
import json, random, sys, time
from collections import deque
import os
if os.environ.get("BACKEND")=="thor":
    from thor import ask
elif os.environ.get("BACKEND")=="clef":
    from clef import ask
else:
    from jev import ask

N = 12
def world(seed):
    r = random.Random(seed)
    g = [['#' if r.random() < 0.22 else '.' for _ in range(N)] for _ in range(N)]
    free = lambda: next((x, y) for x, y in iter(lambda: (r.randrange(N), r.randrange(N)), None) if g[y][x] == '.')
    a = free(); g[a[1]][a[0]] = 'A'
    b = free(); g[b[1]][b[0]] = 'B'
    es = []
    for _ in range(r.randrange(0, 3)):
        e = free(); g[e[1]][e[0]] = 'E'; es.append(e)
    return g, a, b, es

def los(g, p, q):
    (x0, y0), (x1, y1) = p, q
    dx, dy = abs(x1 - x0), -abs(y1 - y0); sx = 1 if x0 < x1 else -1; sy = 1 if y0 < y1 else -1; err = dx + dy
    while (x0, y0) != (x1, y1):
        e2 = 2 * err
        if e2 >= dy: err += dy; x0 += sx
        if e2 <= dx: err += dx; y0 += sy
        if (x0, y0) != (x1, y1) and g[y0][x0] == '#': return False
    return True

def bfs(g, s, t):
    d = {s: 0}; qq = deque([s])
    while qq:
        x, y = qq.popleft()
        if (x, y) == t: return d[(x, y)]
        for nx, ny in ((x+1,y),(x-1,y),(x,y+1),(x,y-1)):
            if 0 <= nx < N and 0 <= ny < N and g[ny][nx] != '#' and (nx, ny) not in d:
                d[(nx, ny)] = d[(x, y)] + 1; qq.append((nx, ny))
    return None

def truth(g, a, b, es):
    cheb = lambda p, q: max(abs(p[0]-q[0]), abs(p[1]-q[1]))
    vis = any(los(g, a, e) and cheb(a, e) <= 6 for e in es)
    path = bfs(g, a, b)
    act = "hide" if vis else ("advance" if path is not None and path <= 10 else "scout")
    dx, dy = b[0]-a[0], b[1]-a[1]
    bearing = ("east" if dx > 0 else "west") if abs(dx) >= abs(dy) else ("south" if dy > 0 else "north")
    reach = path is not None
    return {"enemy_visible": vis, "action": act, "beacon_bearing": bearing, "beacon_reachable": reach}, path

def render(g):
    head = "   " + "".join(f"{x%10}" for x in range(N))
    return "\n".join([head] + [f"{y:2d} " + "".join(row) for y, row in enumerate(g)])

LEGEND = "Top-down 12x12 grid. Row 0 is north (top), column 0 is west (left). '#' wall (blocks movement and sight), '.' floor, 'A' our agent, 'E' enemy, 'B' beacon. Movement is 4-directional."
Q = {
 "enemy_visible": {"type": "noul", "instructions": "Some enemy E has a clear straight line of sight to agent A (no '#' cell on the straight line between them) AND is within 6 cells (Chebyshev distance)."},
 "action": {"type": "choice", "instructions": "Agent A's best action by this rule: hide if an enemy is visible within 6 cells; otherwise advance if the beacon is reachable within 10 steps of 4-directional movement around walls; otherwise scout.",
            "criteria": {"hide": "an enemy can see A within 6 cells", "advance": "no visible enemy and beacon reachable in <=10 steps", "scout": "neither"}},
 "beacon_bearing": {"type": "choice", "instructions": "Dominant compass direction from A to the beacon B (the larger of the horizontal/vertical offsets; ties go east/west).",
                    "criteria": {"north": None, "south": None, "east": None, "west": None}},
 "beacon_reachable": {"type": "noul", "instructions": "There is a path of floor cells from A to B using 4-directional moves that avoids walls."},
}

def state_for(cond, g, a, b, es, other):
    if cond == "blind": return {"legend": LEGEND, "grid": "(not available)"}
    if cond == "facts":
        _, path = truth(g, a, b, es)
        cheb = lambda p, q: max(abs(p[0]-q[0]), abs(p[1]-q[1]))
        lines = [f"Agent A is at column {a[0]}, row {a[1]} (row 0 is north).", f"The beacon B is at column {b[0]}, row {b[1]}."]
        lines.append(f"A walking path to the beacon exists and is {path} steps long." if path is not None else "There is no walking path to the beacon.")
        if not es: lines.append("No enemies are on the map.")
        for i, e in enumerate(es):
            lines.append(f"Enemy {i+1} is {cheb(a,e)} cells away and " + ("CAN see agent A (clear line of sight)." if los(g, a, e) else "cannot see agent A (a wall blocks the line of sight)."))
        return " ".join(lines)
    if cond == "shuffled": return {"legend": LEGEND, "grid": render(other)}
    s = {"legend": LEGEND, "grid": render(g)}
    if cond == "gates":
        _, path = truth(g, a, b, es)
        cheb = lambda p, q: max(abs(p[0]-q[0]), abs(p[1]-q[1]))
        s["computed_by_code"] = {"agent_xy": a, "beacon_xy": b, "enemies": [{"xy": e, "chebyshev": cheb(a, e), "line_of_sight": los(g, a, e)} for e in es],
                                 "bfs_steps_to_beacon": path}
    return s

def score(ans, tr):
    out = {}
    for k, v in tr.items():
        a = ans.get(k, {})
        out[k] = (a.get("noul", 0) >= 0.5) == v if isinstance(v, bool) else a.get("choice") == v
    return out

if __name__ == "__main__":
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 30
    scenes = [world(1000 + i) for i in range(n)]
    res = {c: {k: 0 for k in Q} for c in os.environ.get("CONDS","frame,blind,shuffled,gates").split(",")}
    lat = {c: [] for c in res}; errs = 0; log = []
    for i, (g, a, b, es) in enumerate(scenes):
        tr, _ = truth(g, a, b, es); other = scenes[(i + 7) % n][0]
        for c in res:
            code, resp, ms = ask(state_for(c, g, a, b, es, other), Q)
            if code != 200: errs += 1; print("ERR", c, code, str(resp)[:200]); continue
            sc = score(resp["answers"], tr); lat[c].append(ms)
            for k in sc: res[c][k] += sc[k]
            log.append({"scene": i, "cond": c, "truth": tr, "answers": resp["answers"], "ms": round(ms), "usage": resp.get("usage")})
    json.dump(log, open(os.environ.get("LOGF","e4-log.json"), "w"), indent=1)
    base = {k: max(sum(truth(*s)[0][k] == v for s in scenes) for v in set(truth(*s)[0][k] for s in scenes)) / n for k in Q}
    print(f"n={n} scenes, errors={errs}, model={log[0]['answers'] and 'see log'}")
    print(f"{'cond':9s} " + " ".join(f"{k[:14]:>15s}" for k in Q) + "   p50 ms  p95 ms")
    for c in res:
        L = sorted(lat[c]) or [0]
        print(f"{c:9s} " + " ".join(f"{res[c][k]/n:15.2f}" for k in Q) + f"   {L[len(L)//2]:6.0f}  {L[int(len(L)*.95)-1 if len(L)>1 else 0]:6.0f}")
    print(f"{'majority':9s} " + " ".join(f"{base[k]:15.2f}" for k in Q))
