// The `shared-free` variant's reflex: the same doctrine the variant's move
// gate states, in the form the headless suite can play.
//
// The doctrine, in order:
//
//   1. Fresh contact: charge the enemy with numbers, flee without them.
//   2. Hold only while the beacon is contested.
//   3. Otherwise go to the shared target: the beacon no squad holds.
//
// The difference from the `advance` reflex is the target. `advance` goes to
// the nearest beacon *my squad* does not hold, which on a point-symmetric
// map sends each squad to its own corner. This one goes to the lowest
// beacon *no* squad holds, which is the same beacon for both squads, so they
// meet.
//
// The rule is read off the beacon list, which every agent sees, so both
// squads read the same answer off the same field. That is what makes them
// meet, and it is the same rule the facts builder states.

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

// The shared target: the lowest-numbered beacon no squad holds.
//
// The beacon list is in beacon order, so the first free beacon is the
// lowest-numbered one. Both squads read the same rule off the same field,
// so both go to the same beacon: that is what makes them meet.
//
// A holder below zero is a beacon no squad holds.
fn shared_target(v) {
    let best = -1;
    for i in 0..v.beacons.len() {
        if v.beacons[i].holder < 0 {
            best = i;
            break;
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
