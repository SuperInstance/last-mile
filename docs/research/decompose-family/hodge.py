#!/usr/bin/env python3
"""HodgeRank-style decomposition of a gate's option preferences.

WHAT THIS IS
------------
A "gate" asks one question with N options. A decision model answers with a
probability per option. We turn a set of such answers into a 1-form (a flow)
on the graph whose vertices are the options, and split the flow into

    gradient  (H0)  the part a single potential explains -> ONE SCALE:
                   the options sit on one ranking, the question is one thing.
    curl      (H1)  the part no single potential explains -> CYCLIC:
                   the preferences go round in a loop, so no ranking
                   satisfies everybody, the question MIXES KINDS.
    harmonic  (H2)  the part orthogonal to both -> IRRECONCILABLE.

THE FLOW
--------
For one observation t with option set S_t and probabilities p_t, the pairwise
flow on edge (i, j) is

    w_ij += p_t[i] - p_t[j]        for i, j in S_t

and pairs that never co-occur stay at zero (they are not edges).

THE MATH
--------
Let B be the (|V| x |E|) incidence matrix (head +1, tail -1).

Unweighted metric (the classical orthogonal split, and the one we report):
    E = im(B^T) (gradient space), C = ker(B) (cycle space). E is orthogonal to
    C and E + C spans R^E, so
        omega_g = B^T (B B^T)^+ B omega
        omega_c = P_ker(B) omega
        omega_h = omega - omega_g - omega_c      (exactly 0 on a graph)
    The three energies sum to ||omega||^2; the ratios sum to 1.

CAVEAT ON THE METRIC. The projection above is orthogonal in the STANDARD inner
product, so it does not weight an edge by its flow magnitude: a cycle carrying
0.003 counts as much as one carrying 0.9. To make the scale visible we also
report the CIRCULATION of each triangle,
    c_ijk = w_ij + w_jk + w_ki,
which is identically 0 for a flow that is a gradient and is otherwise exactly
the amount the preferences "go round in a loop" at that triple. We report
max|c| and the sum of squares over triangles.

WHY THERE IS NO WEIGHTED THREE-WAY SPLIT HERE. HodgeRank's Omega-weighted
split needs the simplicial (agent/candidate) setting to be orthogonal. On a
plain graph, E = im(B^T) and C = ker(B) are orthogonal in the standard metric
but NOT in the Omega metric (B Omega c is not 0 for c in ker(B)), so a
weighted three-way orthogonal partition does not exist without changing the
definition of the coexact space. We report the standard split and the
circulations, and say so rather than shipping a number we cannot defend.

H2 IS ZERO ON A GRAPH. im(B^T) and ker(B) span R^E, so the harmonic space of
a graph is empty; HodgeRank's H2 exists only in its bipartite agent/candidate
form. We therefore also report the CONNECTIVITY of the option graph, which is
the operational form of "the options never met".

VERIFICATION
------------
self_test() checks cases with known answers; run it with `python3 hodge.py
selftest`.
"""

import collections
import json
import sys

import numpy as np

EPS = 1e-12


# ---------------------------------------------------------------- graph build
def build_flow(observations, option_sets=None):
    """Aggregate observations into a flow.

    observations: list of dicts {probs: {option: p}, ...}
    option_sets: optional list of option-name lists, one per observation. If
                 omitted, the option set is the keys of that observation's probs.
    Returns (vertices, edges, omega) where edges are ordered (i, j) pairs of
    vertex indices and omega is the flow on each edge.
    """
    if option_sets is None:
        option_sets = [sorted(o["probs"].keys()) for o in observations]

    verts = []
    for s in option_sets:
        for o in s:
            if o not in verts:
                verts.append(o)

    w = {}
    for obs, opts in zip(observations, option_sets):
        p = obs["probs"]
        for a in opts:
            for b in opts:
                if a == b:
                    continue
                key = (a, b)
                w[key] = w.get(key, 0.0) + (p.get(a, 0.0) - p.get(b, 0.0))

    edges, omega = [], []
    for i, a in enumerate(verts):
        for j, b in enumerate(verts):
            if j <= i:
                continue
            if (a, b) in w:
                edges.append((i, j))
                omega.append(w[(a, b)])
            elif (b, a) in w:
                edges.append((i, j))
                omega.append(-w[(b, a)])
    return verts, edges, np.array(omega, dtype=float)


# ------------------------------------------------------- incidence + helpers
def incidence(n_v, edges):
    B = np.zeros((n_v, len(edges)))
    for e, (i, j) in enumerate(edges):
        B[i, e] = 1.0
        B[j, e] = -1.0
    return B


def connected_components(n_v, edges):
    adj = collections.defaultdict(list)
    for i, j in edges:
        adj[i].append(j)
        adj[j].append(i)
    seen, comps = set(), []
    for v in range(n_v):
        if v in seen:
            continue
        q, c = collections.deque([v]), []
        seen.add(v)
        while q:
            x = q.popleft()
            c.append(x)
            for y in adj[x]:
                if y not in seen:
                    seen.add(y)
                    q.append(y)
        comps.append(sorted(c))
    return comps


# ------------------------------------------------------------- decomposition
def decompose(omega, B):
    """Split omega into gradient, curl and harmonic (standard metric)."""
    n, m = B.shape
    if m == 0:
        z = np.zeros(0)
        return {"gradient": 0.0, "curl": 0.0, "harmonic": 0.0, "total": 0.0,
                "omega_g": z, "omega_c": z, "omega_h": z,
                "h0_ratio": 0.0, "h1_ratio": 0.0, "h2_ratio": 0.0}

    total = float(omega @ omega)

    BBt_pinv = np.linalg.pinv(B @ B.T, rcond=1e-12)
    omega_g = B.T @ (BBt_pinv @ (B @ omega))
    Pker = np.eye(m) - B.T @ (BBt_pinv @ B)
    omega_c = Pker @ (omega - omega_g)
    omega_h = omega - omega_g - omega_c

    eg = float(omega_g @ omega_g)
    ec = float(omega_c @ omega_c)
    eh = float(omega_h @ omega_h)
    return {
        "gradient": eg, "curl": ec, "harmonic": eh, "total": total,
        "omega_g": omega_g, "omega_c": omega_c, "omega_h": omega_h,
        "h0_ratio": eg / total if total > EPS else 0.0,
        "h1_ratio": ec / total if total > EPS else 0.0,
        "h2_ratio": eh / total if total > EPS else 0.0,
    }


def circulations(verts, edges, omega):
    """The circulation of every triangle of the option graph.

    c_ijk = w_ij + w_jk + w_ki.  It is identically zero for a flow that is
    the gradient of a single potential, and otherwise it is exactly how much
    the preferences go round in a loop at that triple.

    Sign convention: omega[e] is the flow along edge e oriented from the
    lower-indexed vertex to the higher-indexed one, so w_ij = -w_ji.
    """
    idx = {}
    for e, (i, j) in enumerate(edges):
        idx[(i, j)] = e
        idx[(j, i)] = e

    def w(i, j):
        e = idx[(i, j)]
        return omega[e] if i < j else -omega[e]

    n = len(verts)
    vals = []
    for i in range(n):
        for j in range(i + 1, n):
            if (i, j) not in idx:
                continue
            for k in range(j + 1, n):
                if (j, k) not in idx or (i, k) not in idx:
                    continue
                c = w(i, j) + w(j, k) + w(k, i)
                vals.append((verts[i], verts[j], verts[k], float(c)))
    return vals


def analyse(observations, option_sets=None, label=""):
    verts, edges, omega = build_flow(observations, option_sets)
    B = incidence(len(verts), edges)
    dec = decompose(omega, B)
    comps = connected_components(len(verts), edges)
    circ = circulations(verts, edges, omega)
    return {
        "label": label,
        "vertices": verts, "n_edges": len(edges),
        "n_components": len(comps),
        "components": [[verts[i] for i in c] for c in comps],
        "total_energy": dec["total"],
        "gradient_energy": dec["gradient"],
        "curl_energy": dec["curl"],
        "harmonic_energy": dec["harmonic"],
        "h0_ratio": dec["h0_ratio"],
        "h1_ratio": dec["h1_ratio"],
        "h2_ratio": dec["h2_ratio"],
        "n_triangles": len(circ),
        "max_abs_circulation": max((abs(c[3]) for c in circ), default=0.0),
        "sum_sq_circulation": sum(c[3] ** 2 for c in circ),
        "worst_triangle": max(circ, key=lambda c: abs(c[3])) if circ else None,
    }


# ------------------------------------------------- the bipartite (HodgeRank)
# form
def analyse_bipartite(observations, option_sets=None, label="", scores="rank"):
    """HodgeRank's heterogeneous form: agents on one side, candidates on the other.

    Each observation (one decision model's answer) is an AGENT; each option is
    a CANDIDATE. The graph is the bipartite agent-candidate graph and the edge
    (agent, candidate) carries the agent's preference for that candidate,
    centred on the agent's mean so the agent contributes only its ordering.

    Why this form matters: the cycle space of the bipartite graph contains the
    4-cycles agent1 - c1 - agent2 - c2 - agent1, and the circulation around
    such a cycle is

        (s_1(c1) - s_1(c2)) - (s_2(c1) - s_2(c2)),

    which is zero exactly when the two agents agree on the order of c1 and c2.

    scores="rank": the agent's score for a candidate is derived from the RANK
    of that candidate under that agent (best = high), not from the raw
    probability. That matters: two models with the same ordering but
    different probability strengths give different raw scores and would
    register as a disagreement, while they pick the same option. Ranking is
    the right reduction because the thing being compared is "which option did
    the model choose", and the choice depends only on the ordering.

    scores="prob": the raw probability, centred. Measures disagreement in the
    STRENGTH of a preference, not its order.

    Returns the same shape as analyse(), plus n_agents and n_candidates.
    """
    if option_sets is None:
        option_sets = [sorted(o["probs"].keys()) for o in observations]

    # node list: agents occupy 0..n_a-1, candidates occupy n_a..n_a+n_c-1
    n_a = len(observations)
    cand = []
    for opts in option_sets:
        for o in opts:
            if o not in cand:
                cand.append(o)
    n_c = len(cand)
    cidx = {c: n_a + i for i, c in enumerate(cand)}

    edges, omega = [], []
    for k, (obs, opts) in enumerate(zip(observations, option_sets)):
        p = obs["probs"]
        if scores == "rank":
            # rank 1 = best; score = (n - rank) so a better rank scores higher
            order = sorted(opts, key=lambda o: -p.get(o, 0.0))
            vals = [0.0] * len(opts)
            for pos, o in enumerate(order):
                vals[opts.index(o)] = float(len(opts) - pos)
        else:
            vals = [p.get(o, 0.0) for o in opts]
        mean = sum(vals) / len(vals) if vals else 0.0
        for o, v in zip(opts, vals):
            edges.append((k, cidx[o]))
            omega.append(v - mean)

    omega = np.array(omega, dtype=float)
    B = incidence(n_a + n_c, edges)
    dec = decompose(omega, B)
    comps = connected_components(n_c + len(observations), edges)
    return {
        "label": label,
        "n_agents": len(observations), "n_candidates": n_c,
        "n_edges": len(edges),
        "n_components": len(comps),
        "total_energy": dec["total"],
        "gradient_energy": dec["gradient"],
        "curl_energy": dec["curl"],
        "harmonic_energy": dec["harmonic"],
        "h0_ratio": dec["h0_ratio"],
        "h1_ratio": dec["h1_ratio"],
        "h2_ratio": dec["h2_ratio"],
    }


# --------------------------------------------------------------- self test
def self_test():
    out = []

    def chk(name, value, passed):
        out.append((name, value, passed))

    def close(a, b, tol=1e-10):
        return abs(a - b) <= tol * max(1.0, abs(a), abs(b))

    r = analyse([{"probs": {"a": 0.7, "b": 0.2, "c": 0.1}}], label="one vector")
    chk("one probability vector -> curl == 0", r["curl_energy"], r["curl_energy"] < 1e-18)
    chk("one probability vector -> gradient == total", r["gradient_energy"],
        close(r["gradient_energy"], r["total_energy"]))
    chk("one probability vector -> harmonic == 0", r["harmonic_energy"], r["harmonic_energy"] < 1e-18)
    chk("one probability vector -> every circulation == 0", r["max_abs_circulation"],
        r["max_abs_circulation"] < 1e-15)

    obs = [{"probs": {"a": 1.0, "b": 0.0}}, {"probs": {"b": 1.0, "c": 0.0}},
           {"probs": {"c": 1.0, "a": 0.0}}]
    r = analyse(obs, label="3-cycle")
    chk("3-cycle -> curl > 0", r["curl_energy"], r["curl_energy"] > 1e-6)
    chk("3-cycle -> curl ratio > 0.2", r["h1_ratio"], r["h1_ratio"] > 0.2)
    chk("3-cycle -> energies sum to total",
        r["gradient_energy"] + r["curl_energy"] + r["harmonic_energy"],
        close(r["gradient_energy"] + r["curl_energy"] + r["harmonic_energy"], r["total_energy"]))
    chk("3-cycle -> a triangle circulates", r["max_abs_circulation"], r["max_abs_circulation"] > 1e-6)

    obs = [{"probs": {"a": 0.6, "b": 0.4}}, {"probs": {"x": 0.7, "y": 0.3}}]
    r = analyse(obs, label="disjoint")
    chk("disjoint option sets -> 2 components", r["n_components"], r["n_components"] == 2)

    obs = [{"probs": {"a": 0.6, "b": 0.3, "c": 0.1}}, {"probs": {"c": 0.6, "b": 0.3, "a": 0.1}}]
    r = analyse(obs, label="two models reversed")
    chk("reversed orderings, SAME option set -> curl == 0", r["curl_energy"], r["curl_energy"] < 1e-18)
    chk("reversed orderings, SAME option set -> circulation == 0", r["max_abs_circulation"],
        r["max_abs_circulation"] < 1e-15)

    obs = [{"probs": {"a": 0.6, "b": 0.4}}, {"probs": {"b": 0.6, "c": 0.4}},
           {"probs": {"c": 0.6, "a": 0.4}}]
    r = analyse(obs, label="partial option sets")
    chk("partial option sets -> curl > 0", r["curl_energy"], r["curl_energy"] > 1e-6)
    chk("partial option sets -> a triangle circulates", r["max_abs_circulation"],
        r["max_abs_circulation"] > 1e-6)

    obs = [{"probs": {"a": 0.6, "b": 0.3, "c": 0.1}}, {"probs": {"a": 0.5, "b": 0.3, "c": 0.2}}]
    r = analyse(obs, label="two models agree")
    chk("agreeing orderings -> curl == 0", r["curl_energy"], r["curl_energy"] < 1e-18)

    obs = [{"probs": {"squad": 0.79, "all": 0.03, "agent0": 0.16, "agent1": 0.007,
                      "agent2": 0.004, "agent3": 0.004, "agent4": 0.003,
                      "agent5": 0.003, "agent6": 0.003, "agent7": 0.003}}]
    r = analyse(obs, label="real attention answer")
    chk("one real attention answer -> curl == 0", r["curl_energy"], r["curl_energy"] < 1e-18)
    chk("one real attention answer -> 1 component", r["n_components"], r["n_components"] == 1)
    chk("one real attention answer -> h0 ratio == 1", r["h0_ratio"], close(r["h0_ratio"], 1.0))

    for metric in ("unweighted", "weighted"):
        pass

    # --- the bipartite (HodgeRank heterogeneous) form ------------------------
    # one agent alone -> no cycle -> curl == 0
    r = analyse_bipartite([{"probs": {"a": 0.7, "b": 0.2, "c": 0.1}}], label="one agent")
    chk("[bipartite, rank] one agent -> curl == 0", r["curl_energy"], r["curl_energy"] < 1e-18)

    # two agents with the SAME ordering -> no circulation -> curl == 0
    r = analyse_bipartite([{"probs": {"a": 0.6, "b": 0.3, "c": 0.1}},
                           {"probs": {"a": 0.5, "b": 0.3, "c": 0.2}}], label="agree")
    chk("[bipartite, rank] same ordering -> curl == 0", r["curl_energy"], r["curl_energy"] < 1e-18)

    # the raw-probability form DOES register a difference in strength
    r = analyse_bipartite([{"probs": {"a": 0.6, "b": 0.3, "c": 0.1}},
                           {"probs": {"a": 0.5, "b": 0.3, "c": 0.2}}],
                          label="agree", scores="prob")
    chk("[bipartite, prob] same ordering, different strength -> curl > 0",
        r["curl_energy"], r["curl_energy"] > 1e-6)

    # two agents whose ORDER is reversed -> a 4-cycle circulates -> curl > 0
    r = analyse_bipartite([{"probs": {"a": 0.6, "b": 0.3, "c": 0.1}},
                           {"probs": {"c": 0.6, "b": 0.3, "a": 0.1}}], label="reversed")
    chk("[bipartite, rank] reversed ordering -> curl > 0", r["curl_energy"], r["curl_energy"] > 1e-6)

    # the same pair, on the candidate-only graph -> curl == 0 (the contrast)
    r = analyse([{"probs": {"a": 0.6, "b": 0.3, "c": 0.1}},
                 {"probs": {"c": 0.6, "b": 0.3, "a": 0.1}}], label="reversed, candidates only")
    chk("[candidate-only] reversed agents -> curl == 0", r["curl_energy"], r["curl_energy"] < 1e-18)

    # two agents that never share an option -> the bipartite graph disconnects
    r = analyse_bipartite([{"probs": {"a": 0.6, "b": 0.4}},
                            {"probs": {"x": 0.7, "y": 0.3}}], label="disjoint")
    chk("[bipartite] disjoint option sets -> 2 components", r["n_components"],
        r["n_components"] == 2)

    ok = all(p for _, _, p in out)
    for name, val, passed in out:
        print(f"  [{'PASS' if passed else 'FAIL'}] {name}  (value={val:.3e})")
    return ok


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "selftest":
        print("SELF TEST")
        sys.exit(0 if self_test() else 1)
    print("use: hodge.py selftest")
