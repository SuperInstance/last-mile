//! The Hodge decomposition of a question's preferences.
//!
//! This is HodgeRank (Jiang, Lim, Yao and Ye 2011, *Statistical ranking
//! and combinatorial Hodge theory*), applied to one question's answers.
//!
//! A question's answers are an **edge flow**: every pair of options the
//! models had an opinion about carries how much the models prefer one to
//! the other, and the flow is skew-symmetric — what flows from `i` to `j`
//! is the negative of what flows from `j` to `i`.
//!
//! The Hodge theorem splits such a flow into three orthogonal parts:
//!
//! - the **gradient** is the part that comes from one scale: there is a
//!   score per option and the flow is the difference of the scores. The
//!   options lie on one scale, so the first model's ranking stands and a
//!   second opinion adds nothing.
//! - the **curl** is the part that is locally cyclic: the preferences
//!   around a triangle do not add up. The options do not lie on one
//!   scale — the question mixes kinds of choice — so the question should
//!   be **decomposed** rather than escalated.
//! - the **harmonic** is the part that stays unresolved: a cycle that no
//!   triangle fills, so the evidence does not reach across the options.
//!   Where the graph has more than one component the evidence is
//!   **disconnected** outright, and the honest answer is to refuse and
//!   let the reflex play.
//!
//! The decomposition is computed by explicit orthogonal projection onto
//! the graph's cycle space and onto the boundaries of its triangles,
//! which is the same split the Helmholtzian gives. It is analysis, not
//! simulation: it runs offline on the answers a round logged, and it is
//! exact to floating point.

use std::collections::BTreeMap;

/// What one model said about a question's options.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// The probability of each option the model had, by option key.
    pub probs: BTreeMap<String, f32>,
}

impl View {
    /// A view over the given probabilities.
    pub fn new(probs: BTreeMap<String, f32>) -> Self {
        Self { probs }
    }

    /// The probability of one option, zero when the view did not have it.
    fn prob(&self, option: &str) -> f64 {
        f64::from(self.probs.get(option).copied().unwrap_or(0.0))
    }
}

/// The parts of a question's preference flow, and how much of the flow
/// each one holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hodge {
    /// The energy of the gradient part.
    pub gradient: f64,
    /// The energy of the curl part.
    pub curl: f64,
    /// The energy of the harmonic part.
    pub harmonic: f64,
    /// How many edges the graph had.
    pub edges: usize,
    /// How many connected components the graph had.
    ///
    /// More than one means the evidence does not reach across the
    /// options: the evidence is disconnected.
    pub components: usize,
}

impl Hodge {
    /// The share of the flow each part holds.
    ///
    /// The three parts are orthogonal and they add up to the whole flow,
    /// so the three shares add up to one.
    pub fn fractions(&self) -> (f64, f64, f64) {
        let total = self.gradient + self.curl + self.harmonic;
        if total <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        (
            self.gradient / total,
            self.curl / total,
            self.harmonic / total,
        )
    }

    /// Which part holds the most of the flow.
    pub fn dominant(&self) -> Part {
        let (gradient, curl, harmonic) = self.fractions();
        if curl >= gradient && curl >= harmonic {
            Part::Curl
        } else if harmonic >= gradient && harmonic > curl {
            Part::Harmonic
        } else {
            Part::Gradient
        }
    }
}

/// Which part of a question's flow holds the most of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The options lie on one scale.
    Gradient,
    /// The options do not lie on one scale.
    Curl,
    /// The evidence is disconnected.
    Harmonic,
}

impl std::fmt::Display for Part {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Part::Gradient => f.write_str("gradient"),
            Part::Curl => f.write_str("curl"),
            Part::Harmonic => f.write_str("harmonic"),
        }
    }
}

/// Decompose one question's preferences.
///
/// The options are the keys the question was asked with. The views are
/// the models that answered it: the first model always, the second model
/// where the escalation record has it, and the judge.
///
/// The flow on a pair is the **relative** margin — the difference of the
/// two probabilities over their sum — because a difference is additive
/// and an additive flow is always a gradient: the whole of it, with
/// nothing left for the curl to say. The relative margin is bounded and
/// is not additive, so the parts of a flow that do not come from one
/// scale are visible.
///
/// A pair carries an edge only when the flow on it is **decisive** — at
/// least `threshold` — so the graph has the holes a real question has:
/// a pair nobody had an opinion about is not an edge, and the triangles
/// that are missing are the cycles the curl cannot fill.
pub fn decompose(options: &[String], views: &[View], threshold: f64) -> Hodge {
    let n = options.len();
    if n < 2 || views.is_empty() {
        return Hodge::default();
    }

    // The flow on every pair, and the pairs that carry an edge.
    let mut edges: Vec<(usize, usize, f64)> = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let flow = pair_flow(options, views, i, j);
            if flow.abs() >= threshold {
                edges.push((i, j, flow));
            }
        }
    }
    if edges.is_empty() {
        return Hodge::default();
    }

    let flow: Vec<f64> = edges.iter().map(|(_, _, flow)| *flow).collect();

    // The gradient: the flow that comes from one scale. Its basis is the
    // graph's **stars** — the vector of the edges at each node — because a
    // scale is a number per node and the flow it gives is the difference
    // of two numbers.
    let basis_gradient = orthonormalise(&stars(n, &edges));
    let gradient = project(&flow, &basis_gradient);
    // What is left cannot be explained by any scale: it circulates.
    let circulating: Vec<f64> = flow
        .iter()
        .zip(gradient.iter())
        .map(|(f, g)| f - g)
        .collect();
    // The curl: the circulating part that the triangles can explain.
    let basis_triangles = orthonormalise(&triangle_boundaries(n, &edges));
    let curl = project(&circulating, &basis_triangles);
    // The harmonic: the circulating part the triangles cannot explain.
    let harmonic: Vec<f64> = circulating
        .iter()
        .zip(curl.iter())
        .map(|(c, k)| c - k)
        .collect();

    Hodge {
        gradient: energy(&gradient),
        curl: energy(&curl),
        harmonic: energy(&harmonic),
        edges: edges.len(),
        components: components(n, &edges),
    }
}

/// The flow on one pair: the mean over the views of their relative
/// margin.
fn pair_flow(options: &[String], views: &[View], i: usize, j: usize) -> f64 {
    let mut total = 0.0;
    for view in views {
        let pi = view.prob(&options[i]);
        let pj = view.prob(&options[j]);
        let sum = pi + pj;
        if sum > 0.0 {
            total += (pi - pj) / sum;
        }
    }
    total / views.len() as f64
}

/// The squared norm of a flow.
fn energy(flow: &[f64]) -> f64 {
    flow.iter().map(|f| f * f).sum()
}

/// The **stars** of the graph: at each node, the vector of the edges at
/// it.
///
/// A node's star is +1 on the edges where the node is the smaller end and
/// -1 where it is the larger. The span of the stars is the space of the
/// flows that come from **one scale**: a scale is a number per node, and
/// the flow it gives on an edge is the difference of the two numbers.
///
/// The stars are what the gradient part is built from, and they are the
/// orthogonal complement of the circulations, so what is left after the
/// gradient is taken out cannot be explained by any scale.
fn stars(n: usize, edges: &[(usize, usize, f64)]) -> Vec<Vec<f64>> {
    let m = edges.len();
    let mut out = Vec::with_capacity(n);
    for node in 0..n {
        let mut star = vec![0.0; m];
        for (index, (i, j, _)) in edges.iter().enumerate() {
            if *i == node {
                star[index] += 1.0;
            }
            if *j == node {
                star[index] -= 1.0;
            }
        }
        out.push(star);
    }
    out
}

/// The boundaries of the graph's triangles.
///
/// Each triangle is a cycle of three edges, and its boundary is that
/// cycle: the locally cyclic part of the flow.
fn triangle_boundaries(n: usize, edges: &[(usize, usize, f64)]) -> Vec<Vec<f64>> {
    let m = edges.len();
    // The edges between each pair of nodes.
    let mut between: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (index, (i, j, _)) in edges.iter().enumerate() {
        between.insert((*i, *j), index);
    }
    let mut triangles = Vec::new();
    for a in 0..n {
        for b in (a + 1)..n {
            let Some(ab) = between.get(&(a, b)).copied() else {
                continue;
            };
            for c in (b + 1)..n {
                let (Some(ac), Some(bc)) =
                    (between.get(&(a, c)).copied(), between.get(&(b, c)).copied())
                else {
                    continue;
                };
                let mut boundary = vec![0.0; m];
                // The cycle a -> b -> c -> a.
                boundary[ab] += 1.0;
                boundary[bc] += 1.0;
                boundary[ac] -= 1.0;
                triangles.push(boundary);
            }
        }
    }
    triangles
}

/// An orthonormal basis of a space, by Gram-Schmidt.
fn orthonormalise(vectors: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let mut basis: Vec<Vec<f64>> = Vec::new();
    for vector in vectors {
        let mut v = vector.clone();
        // Twice through: the second pass cleans up what the first lost.
        for _ in 0..2 {
            for b in &basis {
                let dot: f64 = b.iter().zip(v.iter()).map(|(x, y)| x * y).sum();
                for (x, y) in v.iter_mut().zip(b.iter()) {
                    *x -= dot * y;
                }
            }
        }
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 1e-9 {
            for x in v.iter_mut() {
                *x /= norm;
            }
            basis.push(v);
        }
    }
    basis
}

/// The projection of a vector onto a space given by an orthonormal basis.
fn project(vector: &[f64], basis: &[Vec<f64>]) -> Vec<f64> {
    let mut out = vec![0.0; vector.len()];
    for b in basis {
        let dot: f64 = b.iter().zip(vector.iter()).map(|(x, y)| x * y).sum();
        for (x, y) in out.iter_mut().zip(b.iter()) {
            *x += dot * y;
        }
    }
    out
}

/// How many connected components the graph has.
fn components(n: usize, edges: &[(usize, usize, f64)]) -> usize {
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], x: usize) -> usize {
        let mut x = x;
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for (i, j, _) in edges {
        let (ri, rj) = (find(&mut parent, *i), find(&mut parent, *j));
        if ri != rj {
            parent[ri] = rj;
        }
    }
    (0..n).filter(|x| find(&mut parent, *x) == *x).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The options of a question.
    fn options(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// A view over the given probabilities.
    fn view(pairs: &[(&str, f32)]) -> View {
        View::new(pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    }

    #[test]
    fn the_parts_add_up_to_the_whole_flow() {
        // The three parts are orthogonal, so their energies add up to the
        // energy of the flow.
        let opts = options(&["a", "b", "c", "d"]);
        let views = vec![view(&[("a", 0.5), ("b", 0.3), ("c", 0.15), ("d", 0.05)])];
        let h = decompose(&opts, &views, 0.1);
        assert!(h.edges > 0);
        let (g, c, k) = h.fractions();
        assert!((g + c + k - 1.0).abs() < 1e-9, "got {g} {c} {k}");
    }

    #[test]
    fn a_flow_that_lies_on_one_scale_is_all_gradient() {
        // One option far above the others and two that are the same: the
        // margins are the differences of a single scale, so the whole
        // flow is a gradient and nothing is left over.
        let opts = options(&["a", "b", "c"]);
        let views = vec![view(&[("a", 0.9), ("b", 0.05), ("c", 0.05)])];
        let h = decompose(&opts, &views, 0.05);
        let (g, c, k) = h.fractions();
        assert!(g > 0.99, "the gradient holds the flow: {g} {c} {k}");
        assert!(c < 1e-9, "the curl is empty: {c}");
        assert!(k < 1e-9, "the harmonic is empty: {k}");
        assert_eq!(h.dominant(), Part::Gradient);
    }

    #[test]
    fn margins_that_do_not_add_up_leave_a_curl() {
        // The margins of this one do not add up: a over b is 0.2, b over c
        // is 0.333, and a over c is 0.5, which is neither. The options do
        // not lie on one scale, and the part that does not is the curl.
        let opts = options(&["a", "b", "c"]);
        let views = vec![view(&[("a", 0.75), ("b", 0.5), ("c", 0.25)])];
        let h = decompose(&opts, &views, 0.05);
        let (g, c, k) = h.fractions();
        assert!(c > 0.0, "the curl is not empty: {c}");
        assert!(g > c, "the scale is there too: {g} {c} {k}");
        // The graph is complete, so every cycle is a triangle and the
        // harmonic is empty.
        assert!(k < 1e-9, "the harmonic is empty: {k}");
    }

    #[test]
    fn a_ring_without_a_triangle_is_harmonic() {
        // Two tight pairs, far apart: 0.40 and 0.39 are close, 0.22 and
        // 0.21 are close, and the two groups are far from each other. The
        // decisive pairs are exactly the ring between the groups, with no
        // triangle inside it. The cycle cannot be explained by any scale
        // and no triangle fills it, so it is harmonic.
        let opts = options(&["a", "b", "c", "d"]);
        let views = vec![view(&[("a", 0.40), ("b", 0.39), ("c", 0.22), ("d", 0.21)])];
        let h = decompose(&opts, &views, 0.25);
        assert_eq!(h.edges, 4, "the ring has four edges");
        let (g, c, k) = h.fractions();
        assert!(k > 0.0, "the harmonic is not empty: {k}");
        assert!(c < 1e-9, "no triangle fills it: {c}");
        assert!(g > 0.0, "the scale is there too: {g}");
    }

    #[test]
    fn an_option_the_evidence_never_reaches_is_its_own_component() {
        // The evidence does not reach one of the options at all: it is a
        // component of its own, and the flow cannot speak for it.
        let opts = options(&["a", "b", "c", "d"]);
        let views = vec![view(&[("a", 0.9), ("b", 0.8), ("c", 0.5), ("d", 0.49)])];
        let h = decompose(&opts, &views, 0.28);
        assert_eq!(h.components, 2, "one option is on its own");
        assert!(h.edges > 0, "the rest is still connected");
    }

    #[test]
    fn a_question_with_no_opinion_holds_nothing() {
        let opts = options(&["a", "b"]);
        let views = vec![view(&[("a", 0.5), ("b", 0.5)])];
        let h = decompose(&opts, &views, 0.1);
        assert_eq!(h.edges, 0);
        assert_eq!(h.fractions(), (0.0, 0.0, 0.0));
    }

    #[test]
    fn a_complete_graph_holds_no_harmonic() {
        // Every pair is decisive here, so the graph is complete and its
        // triangles fill every cycle: the curl holds the circulation and
        // the harmonic is empty.
        let opts = options(&["a", "b", "c", "d"]);
        let views = vec![view(&[("a", 0.9), ("b", 0.7), ("c", 0.4), ("d", 0.2)])];
        let h = decompose(&opts, &views, 0.05);
        let (g, c, k) = h.fractions();
        assert!(k < 1e-9, "the harmonic is empty: {k}");
        assert!(c > 0.0, "the cycle is not a gradient: {c}");
        assert!(g > 0.0, "the scale is there too: {g}");
    }

    #[test]
    fn two_models_that_disagree_still_lie_on_one_scale() {
        // The relative margins of two models are averaged, and the
        // average of two scales is a scale.
        let opts = options(&["a", "b", "c"]);
        let views = vec![
            view(&[("a", 0.6), ("b", 0.3), ("c", 0.1)]),
            view(&[("a", 0.2), ("b", 0.5), ("c", 0.3)]),
        ];
        let h = decompose(&opts, &views, 0.02);
        assert!(h.edges > 0);
        let (g, _, _) = h.fractions();
        assert!(g > 0.5, "the gradient holds most: {g}");
    }

    #[test]
    fn the_decomposition_of_a_ten_option_question_is_finite() {
        // The whole point of the guard: the decomposition is a fixed
        // amount of arithmetic on a small graph, never a search.
        let opts = options(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        let views = vec![view(&[
            ("a", 0.25),
            ("b", 0.07),
            ("c", 0.058),
            ("d", 0.074),
            ("e", 0.35),
            ("f", 0.10),
            ("g", 0.05),
            ("h", 0.014),
            ("i", 0.013),
            ("j", 0.015),
        ])];
        let h = decompose(&opts, &views, 0.05);
        let (g, c, k) = h.fractions();
        assert!((g + c + k - 1.0).abs() < 1e-9, "got {g} {c} {k}");
    }
}
