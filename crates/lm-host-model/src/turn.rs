pub fn zero_fraction(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 1.0;
    }
    let zeros = samples.iter().filter(|&&x| x == 0.0).count();
    zeros as f32 / samples.len() as f32
}

pub fn mic_is_deaf(samples: &[f32]) -> bool {
    zero_fraction(samples) == 1.0
}

pub struct TurnDetector {
    end_silence_ms: u32,
    max_turn_ms: u32,
    started_ms: Option<u64>,
    last_voice_ms: Option<u64>,
}

impl TurnDetector {
    pub fn new(end_silence_ms: u32, max_turn_ms: u32) -> Self {
        TurnDetector {
            end_silence_ms,
            max_turn_ms,
            started_ms: None,
            last_voice_ms: None,
        }
    }

    pub fn default_for_casey() -> Self {
        Self::new(2500, 6000)
    }

    pub fn on_voice(&mut self, now_ms: u64) {
        if self.started_ms.is_none() {
            self.started_ms = Some(now_ms);
        }
        self.last_voice_ms = Some(now_ms);
    }

    pub fn turn_over(&self, now_ms: u64) -> bool {
        if self.started_ms.is_none() {
            return false;
        }
        let started = self.started_ms.unwrap();
        let last_voice = self.last_voice_ms.unwrap();

        // Turn ends if silence period has exceeded end_silence_ms
        // OR if the maximum turn time has been exceeded
        now_ms.saturating_sub(last_voice) >= self.end_silence_ms as u64
            || now_ms.saturating_sub(started) >= self.max_turn_ms as u64
    }

    pub fn reset(&mut self) {
        self.started_ms = None;
        self.last_voice_ms = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_second_pause_does_not_end_the_turn() {
        let mut detector = TurnDetector::default_for_casey();
        detector.on_voice(0);
        // Pause until 1000ms - this should not end the turn
        assert!(!detector.turn_over(1000));
    }

    #[test]
    fn two_point_four_seconds_does_not_end_it_but_two_point_five_does() {
        let mut detector = TurnDetector::default_for_casey();
        detector.on_voice(0);
        // 2.4 seconds - should not end
        assert!(!detector.turn_over(2400));
        // 2.5 seconds - should end
        assert!(detector.turn_over(2500));
    }

    #[test]
    fn max_turn_cuts_a_long_talker() {
        let mut detector = TurnDetector::default_for_casey();
        // Simulate voice every 100ms up to 7000ms
        for i in 0..70 {
            detector.on_voice(i * 100);
        }
        // At 6000ms, the turn should be over due to max_turn_ms
        assert!(detector.turn_over(6000));
    }

    #[test]
    fn no_turn_before_voice() {
        let detector = TurnDetector::default_for_casey();
        // A fresh detector should never be over
        assert!(!detector.turn_over(0));
        assert!(!detector.turn_over(1000));
    }

    #[test]
    fn deaf_mic_detected() {
        // All zeros -> deaf
        assert!(mic_is_deaf(&[0.0, 0.0, 0.0]));
        // One non-zero sample -> not deaf
        assert!(!mic_is_deaf(&[0.0, 0.0, 1.0]));
        // Empty -> deaf
        assert!(mic_is_deaf(&[]));
    }
}
