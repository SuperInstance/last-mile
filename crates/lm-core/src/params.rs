//! Every tunable of a round, with the spec §11 defaults.
//!
//! D19: any value with more than one workable choice is a parameter. No sim
//! code may tune behaviour with a literal; everything lives here, travels in
//! the round header, and feeds the header hash, so two nodes that agree on
//! the header agree on every rule of the round.

use crate::hash::{Fnv64, StateHash};
use serde::{Deserialize, Serialize};

/// A parameter that refused the round: the offending field by name and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamError {
    pub field: &'static str,
    pub reason: String,
}

impl ParamError {
    fn new(field: &'static str, reason: &str) -> Self {
        Self {
            field,
            reason: reason.to_owned(),
        }
    }
}

/// All tunables of a round (spec §11 / D20–D45).
///
/// Fixed-point fields carry their scale in the name: `*_q16` are Q16
/// fractions (65 536 = 1.0), `*_upt` are units per tick and `*_units` are
/// 1/1024 m units, so nothing here is ever a float.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Params {
    // Timing and scheduling.
    /// Simulation tick rate (30 Hz, D20).
    pub tick_hz: u16,
    /// Minimum decision scheduling lead in ticks (100 ms, D20/D44).
    pub min_lead_ticks: u16,
    /// Maximum decision scheduling lead in ticks (D44).
    pub max_lead_ticks: u16,
    /// Margin added to the measured one-way p99 when computing the decision
    /// lead, in milliseconds (10 ms, D44; spec §11 `sched.lead_estimator`).
    pub lead_margin_ms: u32,
    /// Lateness beyond this window is dropped as stale (D20).
    pub stale_window_ticks: u16,
    /// A state hash is emitted every this many ticks (D20).
    pub hash_interval_ticks: u16,
    /// Log segment length in ticks.
    pub segment_ticks: u32,
    // World shape.
    /// World size in chunks, (x, y); 8×8 chunks of 32 m = 256 m square.
    pub world_chunks: (u8, u8),
    /// Chunk edge in metres; must be a multiple of `cell_m`.
    pub chunk_m: u8,
    /// Navigation cell edge in metres; divides `chunk_m`.
    pub cell_m: u8,
    /// Number of beacons to generate.
    pub beacons: u8,
    // Beacons rules.
    /// Score needed to win the round.
    pub win_score: u32,
    /// Hard wall-clock limit of the round in ticks.
    pub time_limit_ticks: u32,
    // Squads.
    /// Number of squads.
    pub squads: u8,
    /// Agents per squad; agent `squad_size * s` leads squad `s`.
    pub squad_size: u8,
    // Agents.
    /// Movement speed in units per tick (136 ≈ 4 m/s).
    pub speed_upt: i32,
    /// Vision radius in cells.
    pub view_range_cells: u16,
    /// Squad knowledge refresh interval in ticks.
    pub view_update_ticks: u16,
    /// Tag range in units (2 048 = 2 m).
    pub tag_range_units: i32,
    /// Stun duration in ticks (90 ticks = 3 s).
    pub stun_ticks: u32,
    /// Beacon holding radius in units (4 096 = 4 m).
    pub beacon_radius_units: i32,
    // Presence lanes (PoC A / D42, D43).
    /// EMA shift: α = 1/2^shift (2 → α = 1/4).
    pub lane_alpha_shift: u8,
    /// Hysteresis deadband in Q16 (6 554 ≈ 0.10).
    pub lane_deadband_q16: u32,
    // Scripts and intents.
    /// Rhai operation budget per reflex call.
    pub script_fuel: u64,
    /// Default intent time-to-live in ticks.
    pub intent_ttl_ticks: u16,
    /// Route recomputation interval in ticks.
    pub repath_ticks: u16,
    /// Exploration temperature for rolls in Q16 (0 = argmax, D43).
    pub explore_temp_q16: u32,
    // Worldgen L0.
    /// Percentage of cells filled with wall blocks.
    pub wall_density_pct: u8,
    /// Percentage of cells scattered with cover.
    pub cover_density_pct: u8,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            tick_hz: 30,
            min_lead_ticks: 3,
            max_lead_ticks: 9,
            lead_margin_ms: 10,
            stale_window_ticks: 15,
            hash_interval_ticks: 30,
            segment_ticks: 300,
            world_chunks: (8, 8),
            chunk_m: 32,
            cell_m: 2,
            beacons: 3,
            win_score: 1_800,
            time_limit_ticks: 9_000,
            squads: 2,
            squad_size: 4,
            speed_upt: 136,
            view_range_cells: 20,
            view_update_ticks: 6,
            tag_range_units: 2_048,
            stun_ticks: 90,
            beacon_radius_units: 4_096,
            lane_alpha_shift: 2,
            lane_deadband_q16: 6_554,
            script_fuel: 10_000,
            intent_ttl_ticks: 300,
            repath_ticks: 15,
            explore_temp_q16: 0,
            wall_density_pct: 18,
            cover_density_pct: 6,
        }
    }
}

impl Params {
    /// Refuse nonsense before a round starts, naming the first offending
    /// field in declaration order.
    ///
    /// Beyond the cases the plan pins (`tick_hz = 0`, zero squads, a world
    /// smaller than a chunk, `min_lead > max_lead`, a deadband outside Q16,
    /// `cell_m` not dividing `chunk_m`), this also rejects the values that
    /// would make later stages divide by zero (`hash_interval_ticks`,
    /// `view_update_ticks`, `chunk_m`, `cell_m`, `squad_size`) or shift by
    /// 64 or more bits (`lane_alpha_shift`).
    pub fn validate(&self) -> Result<(), ParamError> {
        if self.tick_hz == 0 {
            return Err(ParamError::new("tick_hz", "tick rate must be at least 1"));
        }
        if self.min_lead_ticks == 0 {
            return Err(ParamError::new(
                "min_lead_ticks",
                "minimum lead must be at least 1 tick",
            ));
        }
        if self.min_lead_ticks > self.max_lead_ticks {
            return Err(ParamError::new(
                "min_lead_ticks",
                "min_lead_ticks must not exceed max_lead_ticks",
            ));
        }
        if self.hash_interval_ticks == 0 {
            return Err(ParamError::new(
                "hash_interval_ticks",
                "hash interval must be at least 1 tick",
            ));
        }
        if self.world_chunks.0 == 0 || self.world_chunks.1 == 0 {
            return Err(ParamError::new(
                "world_chunks",
                "the world must be at least one chunk in each direction",
            ));
        }
        if self.chunk_m == 0 {
            return Err(ParamError::new(
                "chunk_m",
                "chunk edge must be at least 1 m",
            ));
        }
        if self.cell_m == 0 {
            return Err(ParamError::new("cell_m", "cell edge must be at least 1 m"));
        }
        if !self.chunk_m.is_multiple_of(self.cell_m) {
            return Err(ParamError::new(
                "cell_m",
                "cell_m must divide chunk_m evenly",
            ));
        }
        if self.squads == 0 {
            return Err(ParamError::new("squads", "a round needs at least 1 squad"));
        }
        if self.squad_size == 0 {
            return Err(ParamError::new(
                "squad_size",
                "each squad needs at least 1 agent",
            ));
        }
        if self.view_update_ticks == 0 {
            return Err(ParamError::new(
                "view_update_ticks",
                "view update interval must be at least 1 tick",
            ));
        }
        if self.lane_alpha_shift >= 64 {
            return Err(ParamError::new(
                "lane_alpha_shift",
                "EMA shift must stay below 64 bits",
            ));
        }
        if self.lane_deadband_q16 > 65_535 {
            return Err(ParamError::new(
                "lane_deadband_q16",
                "deadband is Q16; it must be at most 65 535",
            ));
        }
        Ok(())
    }
}

impl StateHash for Params {
    fn hash_into(&self, h: &mut Fnv64) {
        h.u16(self.tick_hz);
        h.u16(self.min_lead_ticks);
        h.u16(self.max_lead_ticks);
        h.u32(self.lead_margin_ms);
        h.u16(self.stale_window_ticks);
        h.u16(self.hash_interval_ticks);
        h.u32(self.segment_ticks);
        h.write(&[self.world_chunks.0, self.world_chunks.1]);
        h.write(&[self.chunk_m, self.cell_m, self.beacons]);
        h.u32(self.win_score);
        h.u32(self.time_limit_ticks);
        h.write(&[self.squads, self.squad_size]);
        h.i32(self.speed_upt);
        h.u16(self.view_range_cells);
        h.u16(self.view_update_ticks);
        h.i32(self.tag_range_units);
        h.u32(self.stun_ticks);
        h.i32(self.beacon_radius_units);
        h.write(&[self.lane_alpha_shift]);
        h.u32(self.lane_deadband_q16);
        h.u64(self.script_fuel);
        h.u16(self.intent_ttl_ticks);
        h.u16(self.repath_ticks);
        h.u32(self.explore_temp_q16);
        h.write(&[self.wall_density_pct, self.cover_density_pct]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_spec() {
        let p = Params::default();
        assert_eq!(
            (
                p.tick_hz,
                p.min_lead_ticks,
                p.stale_window_ticks,
                p.hash_interval_ticks
            ),
            (30, 3, 15, 30)
        );
        assert_eq!(
            (p.lane_alpha_shift, p.lane_deadband_q16, p.script_fuel),
            (2, 6_554, 10_000)
        );
    }

    #[test]
    fn defaults_validate() {
        assert!(Params::default().validate().is_ok());
    }

    #[test]
    fn lead_margin_is_the_spec_default() {
        // D44: the adaptive lead adds a 10 ms margin to the measured p99.
        assert_eq!(Params::default().lead_margin_ms, 10);
    }

    #[test]
    #[allow(clippy::type_complexity)] // the case table is copied verbatim from the plan
    fn validate_rejects_each_bad_field() {
        let cases: [(&str, fn(&mut Params)); 6] = [
            ("tick_hz", |p| p.tick_hz = 0),
            ("squads", |p| p.squads = 0),
            ("world_chunks", |p| p.world_chunks = (0, 8)),
            ("min_lead_ticks", |p| {
                p.min_lead_ticks = 10;
                p.max_lead_ticks = 9;
            }),
            ("lane_deadband_q16", |p| p.lane_deadband_q16 = 70_000),
            ("cell_m", |p| p.cell_m = 3), /* must divide chunk_m */
        ];
        for (field, f) in cases {
            let mut p = Params::default();
            f(&mut p);
            assert_eq!(p.validate().unwrap_err().field, field);
        }
    }

    #[test]
    fn all_defaults_carry_spec_values() {
        // The full table, so a typo in one field cannot hide behind the
        // sampled assertions above.
        let p = Params::default();
        assert_eq!(
            (
                p.max_lead_ticks,
                p.segment_ticks,
                p.world_chunks,
                p.chunk_m,
                p.cell_m,
                p.beacons
            ),
            (9, 300, (8, 8), 32, 2, 3)
        );
        assert_eq!(
            (p.win_score, p.time_limit_ticks, p.squads, p.squad_size),
            (1_800, 9_000, 2, 4)
        );
        assert_eq!(
            (
                p.speed_upt,
                p.view_range_cells,
                p.view_update_ticks,
                p.tag_range_units,
                p.stun_ticks,
                p.beacon_radius_units
            ),
            (136, 20, 6, 2_048, 90, 4_096)
        );
        assert_eq!(
            (
                p.intent_ttl_ticks,
                p.repath_ticks,
                p.explore_temp_q16,
                p.wall_density_pct,
                p.cover_density_pct
            ),
            (300, 15, 0, 18, 6)
        );
    }

    #[test]
    fn params_hash_is_stable_and_sensitive() {
        let p = Params::default();
        assert_eq!(p.state_hash(), Params::default().state_hash());
        let mut q = p;
        q.cover_density_pct = 7;
        assert_ne!(p.state_hash(), q.state_hash());
    }
}
