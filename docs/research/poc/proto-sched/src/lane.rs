//! Per-agent, per-gate lane conditioning in fixed point integers.
//!
//! The conditioner keeps four Q16 lane scores per agent per gate. Each lane
//! decision eases the scores towards the submitted percentages and the agent's
//! chosen lane only switches when the challenger beats the incumbent by more
//! than the deadband.

/// Smoothing shift: `1/4` of the error is applied per update.
pub const ALPHA_SHIFT: u32 = 2;

/// Deadband in Q16 (0.10). `ceil(0.10 * 65536) = 6554`.
pub const DEADBAND_Q16: i32 = 6554;

/// One Q16 unit.
pub const Q16_ONE: i32 = 65_536;

/// Convert a percentage (0..=100) into Q16.
pub fn pct_to_q16(p: u8) -> i32 {
    (p as i32 * Q16_ONE) / 100
}

/// Lane scores and chosen actions, indexed `[agent][gate]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaneConditioner {
    n_agents: usize,
    n_gates: usize,
    lanes: Vec<Vec<[i32; 4]>>,
    current: Vec<Vec<u8>>,
}

impl LaneConditioner {
    pub fn new(n_agents: usize, n_gates: usize) -> Self {
        let lanes = (0..n_agents)
            .map(|_| (0..n_gates).map(|_| [0i32; 4]).collect())
            .collect();
        let current = (0..n_agents)
            .map(|_| (0..n_gates).map(|_| 0u8).collect())
            .collect();
        LaneConditioner { n_agents, n_gates, lanes, current }
    }

    pub fn n_agents(&self) -> usize {
        self.n_agents
    }

    pub fn n_gates(&self) -> usize {
        self.n_gates
    }

    /// Raw Q16 lane scores of an agent/gate pair.
    pub fn lanes(&self, agent: u16, gate: u8) -> [i32; 4] {
        let (a, g) = (agent as usize, gate as usize);
        if a < self.n_agents && g < self.n_gates {
            self.lanes[a][g]
        } else {
            [0; 4]
        }
    }

    /// Currently chosen lane (the action index) of an agent/gate pair.
    pub fn current(&self, agent: u16, gate: u8) -> u8 {
        let (a, g) = (agent as usize, gate as usize);
        if a < self.n_agents && g < self.n_gates {
            self.current[a][g]
        } else {
            0
        }
    }

    /// Feed a probability vector into the conditioner and return the chosen
    /// lane for that agent/gate.
    ///
    /// * smoothing: `lane += (p_q16 - lane) >> ALPHA_SHIFT`
    /// * switching: the challenger `o` (best smoothed lane) replaces the
    ///   incumbent `c` only when `lane[o] - lane[c] > DEADBAND_Q16`
    pub fn update(&mut self, agent: u16, gate: u8, p: &[u8; 4]) -> u8 {
        let (a, g) = (agent as usize, gate as usize);
        if a >= self.n_agents || g >= self.n_gates {
            return 0;
        }

        let mut l = self.lanes[a][g];
        for i in 0..4 {
            let target = pct_to_q16(p[i]);
            l[i] += (target - l[i]) >> ALPHA_SHIFT;
        }
        self.lanes[a][g] = l;

        let incumbent = self.current[a][g] as usize;
        let mut challenger = 0usize;
        for i in 1..4 {
            if l[i] > l[challenger] {
                challenger = i;
            }
        }
        if l[challenger] - l[incumbent] > DEADBAND_Q16 {
            self.current[a][g] = challenger as u8;
        }
        self.current[a][g]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentage_conversion() {
        assert_eq!(pct_to_q16(0), 0);
        assert_eq!(pct_to_q16(100), 65_536);
        assert_eq!(pct_to_q16(50), 32_768);
    }

    #[test]
    fn smoothing_moves_a_quarter_of_the_error() {
        let mut c = LaneConditioner::new(1, 1);
        // 100% on lane 2 -> lane[2] = (65536 - 0) >> 2 = 16384
        c.update(0, 0, &[0, 0, 100, 0]);
        let l = c.lanes(0, 0);
        assert_eq!(l[2], 16_384);
        assert_eq!(l[0], 0);
        // lane 2 is now far ahead of the incumbent (lane 0): switch
        assert_eq!(c.current(0, 0), 2);
    }

    #[test]
    fn deadband_blocks_small_differences() {
        let mut c = LaneConditioner::new(1, 1);
        // Lane 1 leads lane 0 by 8355 - 8192 = 163, well inside the deadband.
        c.update(0, 0, &[50, 51, 0, 0]);
        let l = c.lanes(0, 0);
        assert_eq!(l[0], 8_192);
        assert_eq!(l[1], 8_355);
        assert_eq!(c.current(0, 0), 0, "a lead inside the deadband must not switch");
    }

    #[test]
    fn switch_requires_more_than_the_deadband() {
        let mut c = LaneConditioner::new(1, 1);
        // 16384 - 0 = 16384 > 6554 -> switch to lane 2
        c.update(0, 0, &[0, 0, 100, 0]);
        assert_eq!(c.current(0, 0), 2);
        assert_eq!(c.lanes(0, 0)[2], 16_384);
        assert_eq!(c.lanes(0, 0)[0], 0);
    }

    #[test]
    fn hysteresis_keeps_the_incumbent_inside_the_deadband() {
        let mut c = LaneConditioner::new(1, 1);
        c.update(0, 0, &[50, 51, 0, 0]);
        assert_eq!(c.current(0, 0), 0);
        // A decisive push on lane 1: 22650 - 14336 = 8314 > 6554 -> switch
        c.update(0, 0, &[50, 100, 0, 0]);
        let l = c.lanes(0, 0);
        assert_eq!(l[0], 14_336);
        assert_eq!(l[1], 22_650);
        assert_eq!(c.current(0, 0), 1);
    }

    #[test]
    fn agents_and_gates_are_independent() {
        let mut c = LaneConditioner::new(2, 2);
        c.update(0, 0, &[0, 0, 100, 0]);
        c.update(1, 1, &[100, 0, 0, 0]);
        assert_eq!(c.current(0, 0), 2);
        assert_eq!(c.current(0, 1), 0);
        assert_eq!(c.current(1, 1), 0);
        assert_eq!(c.lanes(1, 0), [0; 4]);
    }
}
