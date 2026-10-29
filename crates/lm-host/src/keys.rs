//! The host's key bindings: every key, and the one job it does.
//!
//! Every binding lives in this file and nowhere else — a plugin reads its
//! key from here, from the row that names its job. That one rule is what
//! keeps a key from getting two jobs again: `V` once had both push-to-talk
//! and the 2D↔3D flip, and holding it to speak in `--view both` flipped
//! the world under the person. Two tests hold the rule up:
//!
//! * [`no_key_has_two_jobs`] — no key appears in two rows of the table;
//! * [`the_bindings_live_in_one_place`] — no `KeyCode` literal is written
//!   anywhere in the crate but here, and every row of the file is in the
//!   table, so a binding cannot be added beside it either.

use bevy::input::keyboard::KeyCode;

/// One binding: a key, and the one job it does.
pub struct Binding {
    /// The key bound.
    pub key: KeyCode,
    /// The job it does, in words — for the reader, and for the tests,
    /// which name a duplicate by the two jobs that share its key.
    #[cfg_attr(not(test), allow(dead_code))]
    pub job: &'static str,
}

/// `0` lets go of the agent being played (or followed).
pub const LET_GO: Binding = Binding {
    key: KeyCode::Digit0,
    job: "let go of the agent being played",
};

/// `Esc` lets go too: either key releases.
pub const LET_GO_ESCAPE: Binding = Binding {
    key: KeyCode::Escape,
    job: "let go of the agent being played",
};

/// `1` plays as agent 0 … `8` plays as agent 7.
pub const POSSESS: [Binding; 8] = [
    Binding {
        key: KeyCode::Digit1,
        job: "play as agent 0",
    },
    Binding {
        key: KeyCode::Digit2,
        job: "play as agent 1",
    },
    Binding {
        key: KeyCode::Digit3,
        job: "play as agent 2",
    },
    Binding {
        key: KeyCode::Digit4,
        job: "play as agent 3",
    },
    Binding {
        key: KeyCode::Digit5,
        job: "play as agent 4",
    },
    Binding {
        key: KeyCode::Digit6,
        job: "play as agent 5",
    },
    Binding {
        key: KeyCode::Digit7,
        job: "play as agent 6",
    },
    Binding {
        key: KeyCode::Digit8,
        job: "play as agent 7",
    },
];

/// `Tab` toggles between watching and playing the last agent.
pub const TOGGLE_PLAY: Binding = Binding {
    key: KeyCode::Tab,
    job: "toggle between watching and playing",
};

/// `H` holds the ground of the agent being played.
pub const HOLD: Binding = Binding {
    key: KeyCode::KeyH,
    job: "hold the ground",
};

/// `F` follows the agent under the pointer.
pub const FOLLOW: Binding = Binding {
    key: KeyCode::KeyF,
    job: "follow the agent under the pointer",
};

/// `W` pans the camera forward (the 2D map: up; the 3D view: into the
/// screen).
pub const PAN_FORWARD: Binding = Binding {
    key: KeyCode::KeyW,
    job: "pan the camera forward",
};

/// `S` pans the camera back.
pub const PAN_BACK: Binding = Binding {
    key: KeyCode::KeyS,
    job: "pan the camera back",
};

/// `A` pans the camera left.
pub const PAN_LEFT: Binding = Binding {
    key: KeyCode::KeyA,
    job: "pan the camera left",
};

/// `D` pans the camera right.
pub const PAN_RIGHT: Binding = Binding {
    key: KeyCode::KeyD,
    job: "pan the camera right",
};

/// `V` is push-to-talk: hold it and the microphone is open. It is the
/// talk's key alone — see the module docs for how that is kept true.
pub const PUSH_TO_TALK: Binding = Binding {
    key: KeyCode::KeyV,
    job: "push-to-talk (hold to speak)",
};

/// `B` flips the 2D map and the 3D view (`--view both`). It spells the
/// layout it belongs to, and nothing else in the host asks for it.
pub const VIEW_FLIP: Binding = Binding {
    key: KeyCode::KeyB,
    job: "flip the 2D map and the 3D view",
};

/// Every key the host binds, and the one job it does. The two tests below
/// read this table; a system reads its row.
#[cfg_attr(not(test), allow(dead_code))]
pub const BINDINGS: &[&Binding] = &[
    &LET_GO,
    &LET_GO_ESCAPE,
    &POSSESS[0],
    &POSSESS[1],
    &POSSESS[2],
    &POSSESS[3],
    &POSSESS[4],
    &POSSESS[5],
    &POSSESS[6],
    &POSSESS[7],
    &TOGGLE_PLAY,
    &HOLD,
    &FOLLOW,
    &PAN_FORWARD,
    &PAN_BACK,
    &PAN_LEFT,
    &PAN_RIGHT,
    &PUSH_TO_TALK,
    &VIEW_FLIP,
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// No key has two jobs: every key in the table is bound once. This is
    /// the test that fails the day someone gives a second job to `V` (or to
    /// any other key), which is exactly what happened in `--view both`.
    #[test]
    fn no_key_has_two_jobs() {
        for (i, a) in BINDINGS.iter().enumerate() {
            for b in BINDINGS.iter().skip(i + 1) {
                assert!(
                    a.key != b.key,
                    "`{:?}` is bound twice: {} and {}",
                    a.key,
                    a.job,
                    b.job
                );
            }
        }
    }

    /// The bindings live in one place: no `KeyCode` literal is written
    /// anywhere under `src/` but in this file, and this file holds no
    /// binding that the table does not. A binding added beside the table
    /// fails the second half; a binding added inside another plugin fails
    /// the first.
    #[test]
    fn the_bindings_live_in_one_place() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![src.clone()];
        let mut elsewhere = Vec::new();
        let mut here = 0usize;
        while let Some(dir) = files.pop() {
            for entry in fs::read_dir(&dir).expect("the host's src directory") {
                let path = entry.expect("a src entry").path();
                if path.is_dir() {
                    files.push(path);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                    continue;
                }
                let text = fs::read_to_string(&path).expect("a source file");
                // The needle is spelled in pieces so this test's own source
                // does not count itself as a binding.
                let needle = ["KeyCode", "::"].concat();
                let count = text.matches(&needle).count();
                if path.file_stem().and_then(|stem| stem.to_str()) == Some("keys") {
                    here += count;
                } else if count > 0 {
                    elsewhere.push(format!("{} holds {count}", path.display()));
                }
            }
        }
        assert!(
            elsewhere.is_empty(),
            "key bindings outside keys.rs: {}",
            elsewhere.join(", ")
        );
        // One `KeyCode` literal per binding: the file cannot hold a binding
        // the table does not.
        assert_eq!(
            here,
            BINDINGS.len(),
            "keys.rs holds {here} key literals for {} bindings",
            BINDINGS.len()
        );
    }
}
