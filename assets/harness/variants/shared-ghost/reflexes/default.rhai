// The `shared-ghost` variant's reflex: the same doctrine the variant's move
// gate states, in the form the headless suite can play.
//
// The doctrine, in order:
//
//   1. Fresh contact: charge the enemy with numbers, flee without them.
//   2. Hold only while the beacon is contested.
//   3. Otherwise go to the shared target: the beacon you are pulled toward.
//
// The ghost's target is the **hot beacon**: the one the attractor sits on
// now. The hot zone is on one beacon at a time and it walks, moving to the
// next free beacon every 600 ticks. Both squads read the same rule off the
// same field — the tick and the beacon list are the same for both — so
// both are pulled toward the same hot beacon at the same time.
//
// This is the same rule the facts builder states for the `ghost` rule, so
// the headless sweep of this variant is the reflex's play of it. What the
// reflex cannot play is the *representation*: the pull's strength, its
// radius and its falloff are felt by the model, not by the reflex, and
// that is only visible on the Thor.

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

// The hot beacon: the one the attractor sits on this tick.
//
// The hot zone is on ONE beacon at a time and it walks: it moves to the
// next free beacon every 600 ticks, so the place worth fighting over
// moves as the round goes on.
//
// The rule is read off the beacon list and the tick, and both are the same
// for both squads — the tick is the round's, and the beacon list and its
// holders are what every agent sees — so both squads are pulled toward the
// same hot beacon at the same time. That is what makes them meet.
//
// A holder below zero is a beacon no squad holds. When every beacon is
// held the hot zone falls back to the whole list, so it is never on
// nothing.
fn hot_beacon(v) {
    let free = [];
    for i in 0..v.beacons.len() {
        if v.beacons[i].holder < 0 {
            free.push(i);
        }
    }
    if free.len() == 0 {
        for i in 0..v.beacons.len() {
            free.push(i);
        }
    }
    let step = (v.tick / 600) % free.len();
    free[step]
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

    // 2. No contact: go to the beacon the pull names. Never camp on your own.
    let target = hot_beacon(v);
    if target >= 0 {
        return #{ "goto": [v.beacons[target].cell[0], v.beacons[target].cell[1]] };
    }

    // 3. Nothing to do.
    #{ hold: true }
}
