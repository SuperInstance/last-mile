// The `scout` variant's reflex: the same doctrine the variant's move gate
// states, in the form the headless suite can play.
//
// The doctrine, in order:
//
//   1. Fresh contact: charge the enemy with numbers, flee without them.
//   2. If you are the leader of your squad: hold the beacon your squad holds;
//      if your squad holds none, take the nearest beacon your squad does not
//      hold.
//   3. If you are a member: hold the beacon your squad holds; otherwise
//      contest the lowest-numbered beacon no squad holds.
//   4. If there is no beacon to contest, hold.
//
// The difference from the `contest` variant is that the leader stays at home:
// the squad keeps a beacon (and its points) while its members fight for the
// free one, so a losing squad is not wiped off the board.

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

// The index of the beacon I stand on, when my squad holds it; -1 otherwise.
fn on_my_beacon(v, me) {
    let found = -1;
    for i in 0..v.beacons.len() {
        let b = v.beacons[i];
        if b.cell[0] == me[0] && b.cell[1] == me[1] && b.holder == v.me.squad {
            found = i;
        }
    }
    found
}

// The lowest-numbered beacon no squad holds; -1 when there is none.
//
// The beacon list is in beacon order, so the first free beacon is the
// lowest-numbered one. Both squads read the same rule off the same field,
// so both go to the same beacon: that is what makes them meet.
fn lowest_free(v) {
    let best = -1;
    for i in 0..v.beacons.len() {
        if v.beacons[i].holder < 0 {
            best = i;
            break;
        }
    }
    best
}

// The nearest beacon my squad does not hold; -1 when there is none.
fn nearest_ours(v, me) {
    let best = -1;
    let best_d = 0;
    for i in 0..v.beacons.len() {
        let b = v.beacons[i];
        if b.holder == v.me.squad {
            continue;
        }
        let d = chebyshev(me, b.cell);
        if best < 0 || d < best_d {
            best = i;
            best_d = d;
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

    // 2. The beacon my squad holds: the leader holds it. The members do not:
    //    they are the ones who go out and fight.
    if v.me.role == "leader" && on_my_beacon(v, me) >= 0 {
        return #{ hold: true };
    }

    // 3. The target. The leader takes the nearest beacon its own squad does
    //    not hold and keeps it; the members contest the free one.
    let target = -1;
    if v.me.role == "leader" {
        target = nearest_ours(v, me);
    } else {
        target = lowest_free(v);
    }
    if target >= 0 {
        return #{ "goto": [v.beacons[target].cell[0], v.beacons[target].cell[1]] };
    }

    // 4. Nothing to do.
    #{ hold: true }
}
