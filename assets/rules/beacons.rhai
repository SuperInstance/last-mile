// The Beacons rules, re-expressed as a script.
//
// The native ruleset (`rules::beacons`) is the oracle: this script must
// produce exactly the same events, the same agent states and the same
// canonical hashes, tick for tick. A difference is a bug in this script or in
// the layer that applies its effects -- never in the oracle.
//
// The tick runs the phases of the native ruleset, in this order:
//
//   1. The paint: an agent standing in the other team's paint is slowed, and
//      the paint pushes it back every N-th tick of contact.
//   2. Respawn: a stun that has ended puts its agent back on its spawn.
//   3. Tagging: the agents act in the tick's rotating order. An agent that
//      is not stunned tags the first enemy that is not stunned, is within
//      `tag_range_units` and is in line of sight. The target is stunned for
//      `stun_ticks` ticks. A tag never comes back in the same tick, so two
//      agents that can see each other cannot trade tags. The tag also leaves
//      a splat of paint: the tagged agent's cell and the cells around it,
//      within `splat_radius_cells`, in the tagger's colour.
//   4. Holding: a beacon is held by the squad that has a living agent inside
//      `beacon_radius_units` of it while no enemy is inside that radius. A
//      beacon two squads stand in is held by nobody. Every held beacon is
//      worth one point per tick.
//   5. Ending: the round ends when a squad reaches `win_score` or the tick
//      limit runs out.
//
// The paint's rule, in full, is in `crates/lm-core/src/rules/paint.rs`. The
// count of contact ticks is the host's bookkeeping: it rises on every tick an
// agent stands in the other team's paint, and the script reads it already
// counted for the tick it is in. What the paint does about the contact — the
// slow, the push-back — is the script's.
//
// The script spends about half of the fuel one script call gets
// (`script_fuel`, 10 000 operations by default). The tagging scan is the
// cost, and it grows with the square of the roster: a round with a bigger
// roster needs a bigger fuel, which is what `script_fuel` is for.

/// The squared distance between two points, in 1/1024 m units.
fn dist2(a, b) {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    dx * dx + dy * dy
}

/// Whether an agent is stunned at this tick.
///
/// A stun runs from the tick of the tag up to (but not including) the tick it
/// ends on: `stunned_until = now + stun_ticks` holds for `stun_ticks` ticks.
/// A stun end of -1 is no stun at all.
fn is_stunned(stun_end, now) {
    stun_end >= 0 && now < stun_end
}

/// The cells a splat covers: the centre and the cells around it.
///
/// The neighbourhood is a square of Chebyshev radius `radius` -- the metric
/// the grid's sight uses -- clipped to the map, walked in row-major order.
/// This is the same walk the native rules make, so both splat the same cells.
fn splat_cells(cx, cy, radius, w, h) {
    let cells = [];
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let x = cx + dx;
            let y = cy + dy;
            if x < 0 || y < 0 || x >= w || y >= h {
                continue;
            }
            cells.push([x, y]);
        }
    }
    cells
}

/// The rules of one tick: the effects the rules ask for.
fn rules(v) {
    let now = v.tick;
    let n = v.agents.len();
    let nb = v.beacons.len();
    let tag_range2 = v.params.tag_range_units * v.params.tag_range_units;
    let radius2 = v.params.beacon_radius_units * v.params.beacon_radius_units;

    let effects = [];

    // The script's own model of the agents: where they are, and how long they
    // are stunned for. It starts from the world and follows the effects the
    // script asks for, so every phase sees the state the native ruleset
    // sees.
    let pos = [];
    let cell = [];
    let stun_end = [];
    for a in v.agents {
        pos.push(a.pos);
        cell.push(a.cell);
        stun_end.push(a.stunned_until);
    }

    // 1. The paint: an agent standing in the other team's paint is slowed,
    //    and the paint pushes it back every N-th tick of contact.
    //
    //    The paint is read at the start of the tick, before any splat of this
    //    tick is written: a splat is felt from the next tick on. The slow
    //    goes into the agent and the motor of the next tick obeys it; the
    //    push-back happens now, so an agent that steps into the paint is
    //    pushed out at the end of this same tick.
    //
    //    With both knobs at zero the paint plays no rule at all, and the
    //    round is the round it always was.
    let period = 0;
    if v.params.bounce_q8 > 0 {
        period = 256 / v.params.bounce_q8;
        if period < 1 {
            period = 1;
        }
    }
    if v.params.slow_q8 > 0 || v.params.bounce_q8 > 0 {
        for i in 0..n {
            // The paint under the agent's feet: the other team's paint is
            // contact, its own team's paint is not.
            let owner = v.agents[i].paint_owner;
            let contact = owner >= 0 && owner != v.agents[i].squad;
            if contact {
                effects.push(#{ kind: "slow", agent: v.agents[i].id, q8: v.params.slow_q8 });
                // The push-back: on every N-th tick of contact the paint
                // sends the agent back to the cell it came from.
                if period > 0 && v.agents[i].paint_contact % period == 0 {
                    if v.agents[i].from_cell[0] >= 0 {
                        effects.push(#{ kind: "bounce", agent: v.agents[i].id });
                        pos[i] = v.agents[i].from_pos;
                        cell[i] = v.agents[i].from_cell;
                    }
                }
            } else {
                // The slow of last tick is gone: the agent is on ground the
                // paint does not hold.
                effects.push(#{ kind: "slow", agent: v.agents[i].id, q8: 0 });
            }
        }
    }

    // 2. A stun that has ended puts its agent back on its spawn.
    for i in 0..n {
        if stun_end[i] >= 0 && now >= stun_end[i] {
            effects.push(#{ kind: "respawn", agent: v.agents[i].id });
            stun_end[i] = -1;
            pos[i] = v.agents[i].spawn_pos;
            cell[i] = v.agents[i].spawn_cell;
        }
    }

    // 3. Tagging, in the tick's rotating order.
    //
    // `tagged` holds the agents that were tagged during this tick: a tag
    // never comes back in the same tick, so two agents that can see each
    // other cannot trade tags.
    let tagged = [];
    for i in 0..n {
        tagged.push(false);
    }

    for slot in v.order {
        let i = slot;
        // A stunned agent does not act, and neither does an agent that was
        // tagged earlier in this tick.
        if is_stunned(stun_end[i], now) || tagged[i] {
            continue;
        }
        let squad = v.agents[i].squad;
        let victim = -1;
        for j in 0..n {
            if j == i {
                continue;
            }
            if v.agents[j].squad == squad {
                continue;
            }
            if is_stunned(stun_end[j], now) || tagged[j] {
                continue;
            }
            if dist2(pos[i], pos[j]) > tag_range2 {
                continue;
            }
            if !los_clear(cell[i][0], cell[i][1], cell[j][0], cell[j][1]) {
                continue;
            }
            victim = j;
            break;
        }
        if victim >= 0 {
            effects.push(#{ kind: "tag", by: v.agents[i].id, "target": v.agents[victim].id });
            let end = now + v.params.stun_ticks;
            effects.push(#{ kind: "stun_until", agent: v.agents[victim].id, "until": end });
            stun_end[victim] = end;
            tagged[victim] = true;
            // The tag leaves a splat: the tagged agent's cell and the cells
            // around it, in the tagger's colour. The cells are clipped to
            // the map, the way the native rules clip theirs.
            effects.push(#{ kind: "splat",
                            cells: splat_cells(cell[victim][0], cell[victim][1],
                                               v.params.splat_radius_cells,
                                               v.grid[0], v.grid[1]),
                            owner: squad });
        }
    }

    // 4. Holding: a beacon belongs to the squad that has a living agent
    //    inside the radius while no enemy is inside it.
    let scores = v.scores;
    for b in 0..nb {
        let beacon_pos = v.beacons[b].pos;
        // The squads that have a living agent inside the radius.
        let squads_here = [];
        for i in 0..n {
            if is_stunned(stun_end[i], now) {
                continue;
            }
            if dist2(pos[i], beacon_pos) <= radius2 {
                let s = v.agents[i].squad;
                if !squads_here.contains(s) {
                    squads_here.push(s);
                }
            }
        }
        // A beacon two squads stand in is held by nobody.
        let holder = -1;
        if squads_here.len() == 1 && squads_here[0] < v.params.squads {
            holder = squads_here[0];
        }
        if holder != v.beacons[b].holder {
            effects.push(#{ kind: "hold_beacon", beacon: b, squad: holder });
        }
        if holder >= 0 {
            effects.push(#{ kind: "score", squad: holder, points: 1 });
            scores[holder] = scores[holder] + 1;
        }
    }

    // 5. The round ends on the score or on the clock.
    let reached = false;
    for s in scores {
        if s >= v.params.win_score {
            reached = true;
        }
    }
    if reached || now >= v.params.time_limit_ticks {
        effects.push(#{ kind: "round_over" });
    }

    effects
}
