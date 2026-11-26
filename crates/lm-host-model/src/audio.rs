pub enum Duplex {
    Half,
    Full,
}

pub struct DuplexGate {
    mode: Duplex,
    hang_ms: u32,
    suspended_until_ms: u64,
    human_active: bool,
    max_talk_ms: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateAction {
    Play,
    Drop,
}

impl DuplexGate {
    pub fn new(mode: Duplex, hang_ms: u32, max_talk_ms: u32) -> Self {
        Self {
            mode,
            hang_ms,
            suspended_until_ms: 0,
            human_active: false,
            max_talk_ms,
        }
    }

    pub fn on_human_press(&mut self, now_ms: u64) -> GateAction {
        self.human_active = true;
        self.suspended_until_ms = now_ms;
        GateAction::Drop
    }

    pub fn on_human_release(&mut self, now_ms: u64) {
        self.human_active = false;
        self.suspended_until_ms = now_ms + self.hang_ms as u64;
    }

    pub fn on_agent_clip(&mut self, now_ms: u64) -> GateAction {
        if self.human_active || now_ms < self.suspended_until_ms {
            match self.mode {
                Duplex::Half => GateAction::Drop,
                Duplex::Full => GateAction::Play,
            }
        } else {
            GateAction::Play
        }
    }

    pub fn max_talk_ms(&self) -> u32 {
        self.max_talk_ms
    }

    pub fn talk_window(&self, now_ms: u64) -> (u64, u64) {
        let start = now_ms;
        let end = now_ms + self.max_talk_ms as u64;
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_frames_are_dropped_not_queued() {
        let mut gate = DuplexGate::new(Duplex::Half, 400, 6_000);
        let now = 1000;
        gate.on_human_press(now);
        let action = gate.on_agent_clip(now + 100);
        assert_eq!(action, GateAction::Drop);
        gate.on_human_release(now + 200);
        let action = gate.on_agent_clip(now + 500); // Should still be dropped
        assert_eq!(action, GateAction::Drop);
    }

    #[test]
    fn agent_audio_resumes_after_the_400ms_hang() {
        let mut gate = DuplexGate::new(Duplex::Half, 400, 6_000);
        let now = 1000;
        gate.on_human_press(now);
        gate.on_human_release(now + 200);
        let action = gate.on_agent_clip(now + 599); // 399ms after release, should drop
        assert_eq!(action, GateAction::Drop);
        let action = gate.on_agent_clip(now + 600); // 400ms after release, should play
        assert_eq!(action, GateAction::Play);
    }

    #[test]
    fn full_duplex_never_drops() {
        let mut gate = DuplexGate::new(Duplex::Full, 400, 6_000);
        let now = 1000;
        gate.on_human_press(now);
        let action = gate.on_agent_clip(now + 100);
        assert_eq!(action, GateAction::Play);
    }

    #[test]
    fn the_talk_window_is_clipped_to_six_seconds() {
        let gate = DuplexGate::new(Duplex::Half, 400, 6_000);
        let now = 1000;
        let (start, end) = gate.talk_window(now);
        assert_eq!(start, now);
        assert_eq!(end, now + 6_000);
    }
}
