//! What a squad knows and the menus its agents choose from.
//!
//! Task 7 needs `MenuOption` for `Agent::lane_cmd`; Task 8 fills in the rest
//! of this module (SquadKnowledge, ScoreView, Menu, menu()).

use crate::hash::{Fnv64, StateHash};

/// One option of a squad menu (D23/D37).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum MenuOption {
    GoToBeacon(u8),
    Hold,
    FollowLeader,
    Retreat,
}

impl StateHash for MenuOption {
    fn hash_into(&self, h: &mut Fnv64) {
        match self {
            MenuOption::GoToBeacon(b) => {
                h.write(&[0]);
                h.write(&[*b]);
            }
            MenuOption::Hold => h.write(&[1]),
            MenuOption::FollowLeader => h.write(&[2]),
            MenuOption::Retreat => h.write(&[3]),
        }
    }
}
