// The shipped reflex: every agent of every squad runs it.
//
// The contract (see crates/lm-core/src/script.rs):
//
//   * the script defines `fn reflex(v)`, where `v` is the score view as a
//     map with the sorted keys `beacons`, `enemies`, `me`, `score`,
//     `squad` and `tick`;
//   * a cell is an `[x, y]` array;
//   * a beacon's `holder` is the squad holding it, or -1 when it is free;
//   * the answer is `#{ "goto": [x, y] }` (walk to that cell),
//     `#{ hold: true }` (stand still) or `()` (say nothing). The key is
//     quoted because "goto" is a reserved word in Rhai; the runtime reads
//     the key "goto" either way.
//
// The rules, in order:
//
//   1. an enemy within 6 cells: step 4 cells directly away from it,
//      clamped into the map (the motor refuses walls anyway);
//   2. otherwise the nearest beacon no squad of ours holds;
//   3. otherwise hold.
//
// Distances are Chebyshev (the metric of the 8-connected grid). The work
// is split over small functions on purpose: the sandbox caps how deeply
// expressions may nest, and a flat script stays well inside that cap.

fn chebyshev(a, b) {
    let dx = if a[0] > b[0] { a[0] - b[0] } else { b[0] - a[0] };
    let dy = if a[1] > b[1] { a[1] - b[1] } else { b[1] - a[1] };
    if dx > dy { dx } else { dy }
}

// The step away from a threat: four cells, in the direction of the sign.
fn step_away(d) {
    if d > 0 { 4 } else if d < 0 { -4 } else { 0 }
}

// A cell of the map is never negative.
fn clamp_min(v) {
    if v < 0 { 0 } else { v }
}

// The cell four steps away from the enemy, or a hold when the enemy is on
// our own cell and there is no away to run to.
fn flee(me, at) {
    let dx = me[0] - at[0];
    let dy = me[1] - at[1];
    if dx == 0 && dy == 0 {
        return #{ hold: true };
    }
    let x = clamp_min(me[0] + step_away(dx));
    let y = clamp_min(me[1] + step_away(dy));
    #{ "goto": [x, y] }
}

// The index of the nearest beacon no squad of ours holds, or -1.
fn free_beacon(v, me) {
    let best = -1;
    let best_distance = 0;
    for i in 0..v.beacons.len() {
        let beacon = v.beacons[i];
        if beacon.holder < 0 || beacon.holder != v.me.squad {
            let distance = chebyshev(me, beacon.cell);
            if best < 0 || distance < best_distance {
                best = i;
                best_distance = distance;
            }
        }
    }
    best
}

fn reflex(v) {
    let me = v.me.cell;

    // 1. Flee an enemy that is close enough.
    for enemy in v.enemies {
        if chebyshev(me, enemy.cell) <= 6 {
            return flee(me, enemy.cell);
        }
    }

    // 2. Take the nearest beacon that no squad of ours holds.
    let target = free_beacon(v, me);
    if target >= 0 {
        return #{ "goto": [v.beacons[target].cell[0], v.beacons[target].cell[1]] };
    }

    // 3. Nothing to do.
    #{ hold: true }
}
