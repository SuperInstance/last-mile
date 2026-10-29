// The Beacons rules, re-expressed as a script.
//
// The native ruleset (`rules::beacons`) is the oracle: this script must
// produce exactly the same events, the same agent states and the same
// canonical hashes, tick for tick. A difference is a bug in this script or in
// the layer that applies its effects -- never in the oracle.
//
// The tick runs the four phases of the native ruleset, in this order:
//
//   1. Respawn: a stun that has ended puts its agent back on its spawn.
//   2. Tagging: the agents act in the tick's rotating order. An agent that
//      is not stunned tags the first enemy that is not stunned, is within
//      `tag_range_units` and is in line of sight. The target is stunned for
//      `stun_ticks` ticks. A tag never comes back in the same tick, so two
//      agents that can see each other cannot trade tags.
//   3. Holding: a beacon is held by the squad that has a living agent inside
//      `beacon_radius_units` of it while no enemy is inside that radius. A
//      beacon two squads stand in is held by nobody. Every held beacon is
//      worth one point per tick.
//   4. Ending: the round ends when a squad reaches `win_score` or the tick
//      limit runs out.
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

    // 1. A stun that has ended puts its agent back on its spawn.
    for i in 0..n {
        if stun_end[i] >= 0 && now >= stun_end[i] {
            effects.push(#{ kind: "respawn", agent: v.agents[i].id });
            stun_end[i] = -1;
            pos[i] = v.agents[i].spawn_pos;
            cell[i] = v.agents[i].spawn_cell;
        }
    }

    // 2. Tagging, in the tick's rotating order.
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
        }
    }

    // 3. Holding: a beacon belongs to the squad that has a living agent
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

    // 4. The round ends on the score or on the clock.
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
