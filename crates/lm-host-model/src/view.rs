//! The view the host draws: which of the two views, and the switch between
//! them (phase 6 Task 8).
//!
//! The mode is **data, not architecture**: `--view 2d|3d|both` picks the
//! layout the app opens with, and `B` flips between the 2D map and the 3D
//! view while the round runs. A switch is a change to this resource and
//! nothing else — the round, its tick and its recorder are not touched,
//! cannot be touched, because [`ViewSwitch`] holds no round at all. That
//! is the property `view_mode_switches_without_a_restart` states, and the
//! type is the proof: a view that owned the round could reset it; this one
//! cannot.

/// The layout the app opens with, from `--view`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// The 2D map, with its panels: the default until phase 6's done-when.
    Map2d,
    /// The 3D view alone: the low-poly world, no panels.
    View3d,
    /// The 3D view with the egui HUD and the piano roll on top — the
    /// debugger's layout, and the only one that can switch to the 2D map
    /// and back at runtime.
    Both,
}

impl ViewMode {
    /// The mode a command line spells out.
    pub fn parse(text: &str) -> Result<ViewMode, String> {
        match text {
            "2d" => Ok(ViewMode::Map2d),
            "3d" => Ok(ViewMode::View3d),
            "both" => Ok(ViewMode::Both),
            other => Err(format!("`--view` wants 2d, 3d or both, not `{other}`")),
        }
    }

    /// The mode, as it is written on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            ViewMode::Map2d => "2d",
            ViewMode::View3d => "3d",
            ViewMode::Both => "both",
        }
    }
}

/// Which of the two views is drawing right now.
///
/// `Both` is a layout, not a view: it opens on the 3D view (the map is one
/// key away), and the switch below walks between these two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrentView {
    /// The 2D map is drawing.
    Map2d,
    /// The 3D view is drawing.
    View3d,
}

/// The switchable state of the host's views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewSwitch {
    /// The layout the app opened with.
    pub mode: ViewMode,
    /// Which view is drawing now.
    pub current: CurrentView,
}

impl ViewSwitch {
    /// The state an app that opened as `mode` starts in.
    ///
    /// `both` opens on the 3D view — the whole point of the layout is the
    /// 3D view with panels, and the map is one `B` away.
    pub fn new(mode: ViewMode) -> ViewSwitch {
        let current = match mode {
            ViewMode::Map2d => CurrentView::Map2d,
            ViewMode::View3d | ViewMode::Both => CurrentView::View3d,
        };
        ViewSwitch { mode, current }
    }

    /// Flip between the 2D map and the 3D view, and say which is drawing.
    ///
    /// In the single-view layouts there is nothing to flip to, so the
    /// switch stays where it is: the `3d` layout has no map behind it and
    /// the `2d` layout has no 3D view behind it.
    pub fn toggle(&mut self) -> CurrentView {
        if self.mode == ViewMode::Both {
            self.current = match self.current {
                CurrentView::Map2d => CurrentView::View3d,
                CurrentView::View3d => CurrentView::Map2d,
            };
        }
        self.current
    }

    /// Whether the 3D view is the one drawing.
    pub fn showing_3d(&self) -> bool {
        match self.mode {
            ViewMode::Map2d => false,
            ViewMode::View3d | ViewMode::Both => self.current == CurrentView::View3d,
        }
    }

    /// Whether the egui panels (the HUD and the piano roll) are on top.
    pub fn panels(&self) -> bool {
        matches!(self.mode, ViewMode::Map2d | ViewMode::Both)
    }
}

/// The bottom panel's mode: the piano roll, or the conductor's score grown
/// out of it (Vox Score §12, E7).
///
/// The score is a mode *beside* the roll, not a replacement: `E` walks
/// between them while the round runs, and the panel keeps the replay
/// scrubber either way, because both read the same clock. Like the view
/// switch above, this is **data, not architecture**: a flip changes this
/// resource and nothing else — the round, its tick and its recorder are
/// never touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BottomMode {
    /// The piano roll: one lane per agent, the M1 debugger.
    Roll,
    /// The conductor's score: one system of staves per agent.
    Score,
}

impl BottomMode {
    /// The mode a command line spells out.
    pub fn parse(text: &str) -> Result<BottomMode, String> {
        match text {
            "roll" => Ok(BottomMode::Roll),
            "score" => Ok(BottomMode::Score),
            other => Err(format!("`--bottom` wants roll or score, not `{other}`")),
        }
    }

    /// The mode, as it is written on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            BottomMode::Roll => "roll",
            BottomMode::Score => "score",
        }
    }
}

/// Which of the bottom panel's modes is drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BottomSwitch {
    /// The mode the panel is in.
    pub mode: BottomMode,
}

impl Default for BottomSwitch {
    /// The panel opens on the roll, the debugger it has always been.
    fn default() -> BottomSwitch {
        BottomSwitch {
            mode: BottomMode::Roll,
        }
    }
}

impl BottomSwitch {
    /// The state an app opens with.
    pub fn new(mode: BottomMode) -> BottomSwitch {
        BottomSwitch { mode }
    }

    /// Walk between the roll and the score, and say which is drawing.
    pub fn toggle(&mut self) -> BottomMode {
        self.mode = match self.mode {
            BottomMode::Roll => BottomMode::Score,
            BottomMode::Score => BottomMode::Roll,
        };
        self.mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_loop::HostLoop;
    use lm_core::header::Header;

    /// A round five ticks in, unplayed by anyone.
    fn round_five_ticks_in() -> HostLoop {
        let mut host =
            HostLoop::new(Header::default_round(1, lm_tools::runner::DEFAULT_REFLEX)).unwrap();
        for _ in 0..5 {
            host.tick();
        }
        host
    }

    #[test]
    fn view_mode_switches_without_a_restart() {
        let mut host = round_five_ticks_in();
        let tick = host.sim().tick();
        let hash = host.sim().state_hash();

        // A switch in each direction, in the layout that has both views.
        let mut switch = ViewSwitch::new(ViewMode::Both);
        assert_eq!(switch.current, CurrentView::View3d, "`both` opens on 3D");
        assert_eq!(switch.toggle(), CurrentView::Map2d);
        assert!(
            !switch.showing_3d() && switch.panels(),
            "the map with panels"
        );
        assert_eq!(switch.toggle(), CurrentView::View3d);
        assert!(switch.showing_3d() && switch.panels());

        // The switch touched nothing: the round is where it was, to the
        // hash. The view is a view, never a second source of truth.
        assert_eq!(host.sim().tick(), tick);
        assert_eq!(host.sim().state_hash(), hash);

        // And the round runs on through switches, from the tick it held.
        switch.toggle();
        for _ in 0..5 {
            host.tick();
        }
        assert_eq!(host.sim().tick(), tick + 5);
        switch.toggle();
        for _ in 0..5 {
            host.tick();
        }
        assert_eq!(host.sim().tick(), tick + 10, "no switch cost a tick");
    }

    #[test]
    fn single_view_layouts_do_not_flip() {
        // `2d` has no 3D view behind it; `3d` has no map behind it. The
        // switch says so by staying put rather than promising a view that
        // was never installed.
        let mut flat = ViewSwitch::new(ViewMode::Map2d);
        assert_eq!(flat.current, CurrentView::Map2d);
        assert!(!flat.showing_3d());
        assert!(flat.panels(), "the 2D layout keeps its panels");
        assert_eq!(flat.toggle(), CurrentView::Map2d);

        let mut deep = ViewSwitch::new(ViewMode::View3d);
        assert_eq!(deep.current, CurrentView::View3d);
        assert!(deep.showing_3d());
        assert!(!deep.panels(), "the 3D layout has no panels");
        assert_eq!(deep.toggle(), CurrentView::View3d);
    }

    #[test]
    fn the_command_line_spells_the_modes() {
        assert_eq!(ViewMode::parse("2d"), Ok(ViewMode::Map2d));
        assert_eq!(ViewMode::parse("3d"), Ok(ViewMode::View3d));
        assert_eq!(ViewMode::parse("both"), Ok(ViewMode::Both));
        assert!(ViewMode::parse("4d").is_err());
        assert!(ViewMode::parse("").is_err());
        for mode in [ViewMode::Map2d, ViewMode::View3d, ViewMode::Both] {
            assert_eq!(
                ViewMode::parse(mode.as_str()),
                Ok(mode),
                "every mode round-trips through its own spelling"
            );
        }
    }

    /// The bottom switch walks between the roll and the score and touches
    /// nothing but itself — the same property the view switch states, said
    /// for the panel under the round.
    #[test]
    fn the_bottom_switch_flips_without_touching_the_round() {
        let mut host = round_five_ticks_in();
        let tick = host.sim().tick();
        let hash = host.sim().state_hash();

        let mut bottom = BottomSwitch::new(BottomMode::Roll);
        assert_eq!(bottom.toggle(), BottomMode::Score);
        assert_eq!(bottom.toggle(), BottomMode::Roll);
        assert_eq!(bottom.mode, BottomMode::Roll);

        // The switch touched nothing: the round is where it was, to the hash.
        assert_eq!(host.sim().tick(), tick);
        assert_eq!(host.sim().state_hash(), hash);

        // And the round runs on through flips, from the tick it held.
        bottom.toggle();
        for _ in 0..5 {
            host.tick();
        }
        assert_eq!(host.sim().tick(), tick + 5, "no flip cost a tick");
    }

    /// Both spellings of the bottom mode round-trip through the command
    /// line's words, and nothing else is accepted.
    #[test]
    fn the_bottom_modes_round_trip() {
        assert_eq!(BottomMode::parse("roll"), Ok(BottomMode::Roll));
        assert_eq!(BottomMode::parse("score"), Ok(BottomMode::Score));
        assert!(BottomMode::parse("lanes").is_err());
        assert!(BottomMode::parse("").is_err());
        for mode in [BottomMode::Roll, BottomMode::Score] {
            assert_eq!(BottomMode::parse(mode.as_str()), Ok(mode));
        }
    }
}
