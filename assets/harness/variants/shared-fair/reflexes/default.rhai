// The `shared-fair` variant's reflex: the same doctrine the variant's move
// gate states, in the form the headless suite can play.
//
// The doctrine, in order:
//
//   1. Fresh contact: charge the enemy with numbers, flee without them.
//   2. Otherwise go to the shared target: the beacon you are pulled toward.
//   3. If the other squad holds the shared target, go to your second
//      objective: the nearest unheld beacon.
//
// The ghost's target is the **hot beacon**: the one the attractor sits on
// now. The hot zone is on one beacon at a time and it walks, moving on
// every 600 ticks. Both squads read the same rule off the same field — the
// tick and the beacon list are the same for both — so both are pulled
// toward the same hot beacon at the same time.
//
// **The fair walk.** The zone walks the beacons in the order of their
// angle about the map's centre, and it visits every beacon, taken or not.
// Two reasons, both about fairness:
//
//   * the 180-degree rotation adds half a turn to every angle, so the
//     sorted list of a rotated map is the rotated list of the map: the
//     order cannot favour a side of the map;
//   * the zone's place is a function of the tick and the beacons' places
//     alone, so the round's own history cannot feed back into the zone.
//     The `shared-ghost` walk drops a taken beacon from its list, which
//     shifts every later step, and that is how the colour bias grew: over
//     100 seeds it decided 64 rounds to squad 0 and 36 to squad 1.
//
// The middle of the beacons is the middle of the map, because the beacons
// come in 180-degree rotated pairs. The offsets are scaled by the number of
// beacons, so the middle of an even number of cells is exact: no rounding
// and no floats.
//
// This is the same rule the facts builder states for the `ghost` rule with
// `walk = "around"`, so the headless sweep of this variant is the reflex's
// play of it. What the reflex cannot play is the *representation*: the
// pull's strength, its radius and its falloff are felt by the model, not by
// the reflex, and that is only visible on the Thor.
//
// One thing the reflex cannot play either: the walking path. The facts
// builder measures "the nearest" by the walk that really costs (JPS), while
// the reflex has no paths and measures it by the straight line. That
// difference is the same one the shipped sweep has for the doctrine's own
// target.

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

// Each beacon's offset from the middle of the beacons, scaled by the number
// of beacons. The scaling keeps the middle of an even number of cells
// exact, so the offsets are whole numbers whatever the columns and rows
// are.
fn offsets(v) {
    let n = v.beacons.len();
    let sum_x = 0;
    let sum_y = 0;
    for i in 0..n {
        sum_x += v.beacons[i].cell[0];
        sum_y += v.beacons[i].cell[1];
    }
    let out = [];
    for i in 0..n {
        out.push([n * v.beacons[i].cell[0] - sum_x, n * v.beacons[i].cell[1] - sum_y]);
    }
    out
}

// Which half of the turn about the middle an offset is in: above it, or
// below it. The turn starts at the east and runs anticlockwise.
fn half(d) {
    if d[1] > 0 || (d[1] == 0 && d[0] > 0) { 0 } else { 1 }
}

// Which of two offsets comes first in the turn about the middle. Within a
// half the cross product says which is anticlockwise of which; two offsets
// on the same ray are ordered by their distance from the middle, which the
// rotation leaves alone.
fn angle_less(a, b) {
    let ha = half(a);
    let hb = half(b);
    if ha != hb { return ha < hb; }
    let cross = a[0] * b[1] - a[1] * b[0];
    if cross != 0 { return cross > 0; }
    let da = a[0] * a[0] + a[1] * a[1];
    let db = b[0] * b[0] + b[1] * b[1];
    da < db
}

// The beacons in the order of their angle about the middle of the beacons,
// starting at the beacon nearest that middle.
//
// The walk has to start at the middle. A beacon is held by the squad that
// has an agent inside it while no enemy is inside it, and a beacon two
// squads stand in is held by nobody: so whoever reaches the zone first
// holds it alone and scores alone until the other squad arrives. That
// lead is a lead at the next beacon too. Starting the walk on the far
// side of the map therefore hands the round to one squad - over 100 seeds,
// the walk that started on the east side decided 88 rounds to squad 1 and
// 12 to squad 0 - so the walk starts where neither squad is nearer.
fn around_order(v) {
    let offs = offsets(v);
    let order = [];
    for i in 0..v.beacons.len() {
        order.push(i);
    }
    for i in 1..v.beacons.len() {
        let j = i;
        while j > 0 && angle_less(offs[order[j]], offs[order[j - 1]]) {
            let tmp = order[j];
            order[j] = order[j - 1];
            order[j - 1] = tmp;
            j -= 1;
        }
    }
    // The beacon nearest the middle of the beacons: the lowest number
    // breaking a tie, which is the same rule the facts builder uses for
    // the doctrine's own nearest beacon. The beacons are walked in their
    // own numbers here, not in the angular order, so that the tie is
    // broken the same way the facts builder breaks it.
    let n = v.beacons.len();
    let best = [0, distance(offs[0])];
    for i in 1..n {
        let d = distance(offs[i]);
        if d < best[1] {
            best[0] = i;
            best[1] = d;
        }
    }
    // Turn the list round so it starts at that beacon.
    let start = 0;
    for i in 0..n {
        if order[i] == best[0] { start = i; }
    }
    let turned = [];
    for i in 0..n {
        turned.push(order[(start + i) % n]);
    }
    turned
}

// The squared distance of an offset from the middle.
fn distance(d) {
    d[0] * d[0] + d[1] * d[1]
}

// The hot beacon: the one the attractor sits on this tick.
//
// The hot zone is on ONE beacon at a time and it walks: it moves every 600
// ticks, so the place worth fighting over moves as the round goes on. It
// walks every beacon, taken or not, so its place is a function of the tick
// and the beacons' places alone.
//
// The rule is read off the beacon list and the tick, and both are the same
// for both squads — the tick is the round's, and the beacon list and its
// holders are what every agent sees — so both squads are pulled toward the
// same hot beacon at the same time. That is what makes them meet.
fn hot_beacon(v) {
    let order = around_order(v);
    if order.len() == 0 { return -1; }
    let step = (v.tick / 600) % order.len();
    order[step]
}

// The next stop of the zone: the beacon after the hot one in the same walk.
//
// It is the same for every agent of every squad, because the tick and the
// beacon list are what every agent sees.
//
// The squad that has lost the shared target is sent here. It walks to
// where the zone is going and is there when the zone arrives, so the
// fight carries on where the zone is and the loser is in it: a beacon
// two squads stand in is held by nobody, so a squad that waits on the
// beacon the zone has left scores nothing, and one that is at the next
// stop when the zone arrives holds it.
fn next_stop(v) {
    let order = around_order(v);
    if order.len() == 0 { return -1; }
    let step = ((v.tick / 600) + 1) % order.len();
    order[step]
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

    // 2. The shared target: the beacon the pull names.
    let hot = hot_beacon(v);
    if hot >= 0 {
        // 3. The other squad holds it: this squad has lost the beacon, and
        //    its second objective is the next stop of the zone.
        if v.beacons[hot].holder >= 0 && v.beacons[hot].holder != v.me.squad {
            let next = next_stop(v);
            if next >= 0 {
                return #{ "goto": [v.beacons[next].cell[0], v.beacons[next].cell[1]] };
            }
        }
        return #{ "goto": [v.beacons[hot].cell[0], v.beacons[hot].cell[1]] };
    }

    // 4. Nothing to do.
    #{ hold: true }
}
