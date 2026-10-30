// The `shared-centre` variant's reflex: the same doctrine the variant's move
// gate states, in the form the headless suite can play.
//
// The doctrine, in order:
//
//   1. Fresh contact: charge the enemy with numbers, flee without them.
//   2. Hold only while the beacon is contested.
//   3. Otherwise go to the shared target: the most central of the beacons.
//
// The difference from the `advance` reflex is the target. `advance` goes to
// the nearest beacon *my squad* does not hold, which on a point-symmetric
// map sends each squad to its own corner. This one goes to the most central
// beacon, which is the same beacon for both squads, so they meet — and
// unlike `shared-free`'s target, it does not move as the map is taken, so
// the two squads are sent to the same place for the whole round.
//
// The beacon is the one whose total distance to the others is smallest, with
// the lowest number breaking a tie. It is computed from the beacon list
// alone, which every agent sees, and it is the same rule the facts builder
// states.

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

// How many of my squadmates (not me, not stunned) stand within 4 cells.
fn allies_near(v, me) {
    let n = 0;
    for mate in v.squad {
        if mate.id != v.me.id && !mate.stunned && chebyshev(me, mate.cell) <= 4 {
            n += 1;
        }
    }
    n
}

// Fresh enemy sightings within 6 cells: [count, index of the nearest, its distance].
fn threats(v, me) {
    let count = 0;
    let nearest = -1;
    let nearest_d = 0;
    for i in 0..v.enemies.len() {
        let e = v.enemies[i];
        if e.age_ticks <= 6 {
            let d = chebyshev(me, e.cell);
            if d <= 6 {
                count += 1;
                if nearest < 0 || d < nearest_d {
                    nearest = i;
                    nearest_d = d;
                }
            }
        }
    }
    [count, nearest, nearest_d]
}

// The shared target: the most central beacon — the one whose total distance
// to the others is smallest, the lowest number breaking a tie.
fn shared_target(v) {
    let best = -1;
    let best_total = 0;
    for i in 0..v.beacons.len() {
        let total = 0;
        for j in 0..v.beacons.len() {
            total += chebyshev(v.beacons[i].cell, v.beacons[j].cell);
        }
        if best < 0 || total < best_total {
            best = i;
            best_total = total;
        }
    }
    best
}

fn reflex(v) {
    let me = v.me.cell;

    // 1. Fresh contact: charge with numbers, flee without.
    let t = threats(v, me);
    if t[0] > 0 {
        let target = v.enemies[t[1]].cell;
        if allies_near(v, me) + 1 >= t[0] {
            return #{ "goto": [target[0], target[1]] };
        }
        return flee(me, target);
    }

    // 2. No contact: go to the shared target. Never camp on your own.
    let target = shared_target(v);
    if target >= 0 {
        return #{ "goto": [v.beacons[target].cell[0], v.beacons[target].cell[1]] };
    }

    // 3. Nothing to do.
    #{ hold: true }
}
