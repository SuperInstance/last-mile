//! Sandboxed Rhai reflexes: the fast path every agent runs every tick.
//!
//! A reflex is a small script that turns the world as one agent's squad
//! knows it (the [`ScoreView`]) into one motor command. It runs inside
//! `lm-core` on every node, so it costs no traffic and replays
//! deterministically (spec §5, D21).
//!
//! The sandbox has three walls:
//!
//! * **No floats.** The engine is built with `no_float`, so a script that
//!   even mentions a float literal fails to compile — a reflex can never
//!   introduce a value the fixed-point core cannot represent.
//! * **Fuel.** Every call gets an operation budget (`Params::script_fuel`).
//!   A script that never returns runs out of fuel and is cut off for that
//!   tick; the default motor behaviour continues.
//! * **A narrow contract.** The script gets one map and must answer with
//!   `()`, `#{ "goto": [x, y] }` or `#{ hold: true }`. Anything else is
//!   refused, never guessed at (D26: a refusal is an outcome).
//!
//! A broken script therefore costs its agent one tick and nothing else:
//! a parse error stops the round at setup, a runtime error or a garbage
//! return disables the reflex for that tick.
//!
//! The key of the walking answer is `goto`, but a script has to spell it
//! `#{ "goto": [x, y] }`: `goto` is a reserved word in Rhai and cannot be
//! a bare property name. The runtime reads the key `goto` either way.

use crate::agent::MotorCmd;
use crate::header::Role;
use crate::view::{AgentSnap, ScoreView};
use crate::world::CellXY;
use rhai::{Dynamic, Engine, EvalAltResult, Map, Scope, AST};
use std::collections::BTreeMap;

/// The name of the entry point every reflex script defines.
pub const ENTRY_POINT: &str = "reflex";

/// The holder value of a beacon nobody holds.
const HOLDER_NONE: i64 = -1;

/// How many calls a script may nest, and how deeply an expression may
/// nest.
///
/// Rhai's own defaults differ between a debug and a release build (8 and
/// 64 calls, 16 and 32 expression levels), so a script could parse in
/// release and fail to parse in debug — the same source, two different
/// rounds. The sandbox therefore pins the limits, and the fuel (not the
/// depth) is what actually bounds the work.
const MAX_CALL_LEVELS: usize = 64;
const MAX_EXPR_DEPTH: usize = 64;
const MAX_FUNCTION_EXPR_DEPTH: usize = 32;

/// What a reflex said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReflexOut {
    /// A motor command: walk to a cell, or hold.
    Cmd(MotorCmd),
    /// The reflex said nothing: the agent's default behaviour plays.
    Nothing,
    /// The reflex ran out of fuel and was cut off for this tick.
    OutOfFuel,
    /// The reflex failed while running.
    Error(String),
    /// The reflex answered something the contract does not allow.
    Refused(String),
}

/// Why a script could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptError {
    /// The source did not compile (syntax error, float literal, ...).
    Parse(String),
    /// The source compiled but does not define the entry point.
    NoEntryPoint(String),
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScriptError::Parse(msg) => write!(f, "script does not parse: {msg}"),
            ScriptError::NoEntryPoint(name) => {
                write!(f, "script {name:?} defines no fn {ENTRY_POINT}(v)")
            }
        }
    }
}

impl std::error::Error for ScriptError {}

/// The runtime a round runs its reflexes on.
///
/// The trait is the seam a later runtime (WASM) plugs into, so the sim
/// never names Rhai directly (spec §5).
pub trait ScriptRuntime {
    /// Load the script `src` under the name `name`.
    ///
    /// This happens at round setup, so a script that does not compile
    /// stops the round before it starts.
    fn load(&mut self, name: &str, src: &str) -> Result<(), ScriptError>;

    /// Run the reflex `name` on `view` with `fuel` operations of budget.
    ///
    /// The budget belongs to this call alone: running out of fuel cuts the
    /// script off for this tick and nothing else.
    fn reflex(&mut self, name: &str, view: &ScoreView, fuel: u64) -> ReflexOut;
}

/// The Rhai runtime: one compiled script per name, one fuel budget per
/// call.
pub struct RhaiRuntime {
    engine: Engine,
    scripts: BTreeMap<String, AST>,
}

impl RhaiRuntime {
    /// A runtime with no scripts loaded.
    pub fn new() -> RhaiRuntime {
        let mut engine = Engine::new();
        engine.set_max_call_levels(MAX_CALL_LEVELS);
        engine.set_max_expr_depths(MAX_EXPR_DEPTH, MAX_FUNCTION_EXPR_DEPTH);
        RhaiRuntime {
            engine,
            scripts: BTreeMap::new(),
        }
    }
}

impl Default for RhaiRuntime {
    fn default() -> Self {
        RhaiRuntime::new()
    }
}

impl ScriptRuntime for RhaiRuntime {
    fn load(&mut self, name: &str, src: &str) -> Result<(), ScriptError> {
        let ast = self
            .engine
            .compile(src)
            .map_err(|e| ScriptError::Parse(e.to_string()))?;
        let has_entry = ast
            .iter_functions()
            .any(|f| f.name == ENTRY_POINT && f.params.len() == 1);
        if !has_entry {
            return Err(ScriptError::NoEntryPoint(name.to_owned()));
        }
        self.scripts.insert(name.to_owned(), ast);
        Ok(())
    }

    fn reflex(&mut self, name: &str, view: &ScoreView, fuel: u64) -> ReflexOut {
        // The budget is this call's alone: Rhai counts the operations of one
        // evaluation and starts fresh on the next. A fuel of zero is read as
        // "no limit" by Rhai, which would let a runaway script hang the
        // sim, so zero buys the script nothing at all.
        self.engine.set_max_operations(fuel.max(1));
        let Some(ast) = self.scripts.get(name) else {
            return ReflexOut::Refused(format!("no reflex named {name:?}"));
        };
        let mut scope = Scope::new();
        let view_map = view_map(view);
        match self
            .engine
            .call_fn::<Dynamic>(&mut scope, ast, ENTRY_POINT, (view_map,))
        {
            Ok(answer) => interpret(answer),
            Err(err) => classify(&err),
        }
    }
}

/// Turn a runtime failure into an outcome: only the fuel limit is a fuel
/// problem, everything else is an error.
fn classify(err: &EvalAltResult) -> ReflexOut {
    match err {
        EvalAltResult::ErrorTooManyOperations(_) => ReflexOut::OutOfFuel,
        other => ReflexOut::Error(other.to_string()),
    }
}

/// Read the answer of a reflex.
///
/// The contract allows exactly three answers: `()` (say nothing),
/// `#{ "goto": [x, y] }` (walk to a cell) and `#{ hold: true }` (stand
/// still). Everything else is refused — the motor never guesses (D26).
/// The answer map carries exactly one key: a second key, or a key the
/// contract does not name, is refused as well.
fn interpret(answer: Dynamic) -> ReflexOut {
    if answer.is_unit() {
        return ReflexOut::Nothing;
    }
    let type_name = answer.type_name();
    let Some(map) = answer.try_cast::<Map>() else {
        return ReflexOut::Refused(format!(
            "reflex returned a {type_name}; expected (), #{{goto: [x, y]}} or #{{hold: true}}"
        ));
    };
    if map.len() != 1 {
        return ReflexOut::Refused(format!(
            "reflex returned {} keys; expected exactly one of goto or hold",
            map.len()
        ));
    }
    if let Some(goto) = map.get("goto") {
        return match cell_of(goto) {
            Some(c) => ReflexOut::Cmd(MotorCmd::GoTo(c)),
            None => ReflexOut::Refused("reflex returned a goto that is not a cell".to_owned()),
        };
    }
    if let Some(hold) = map.get("hold") {
        return match hold.as_bool() {
            Ok(true) => ReflexOut::Cmd(MotorCmd::Hold),
            _ => ReflexOut::Refused("reflex returned a hold that is not true".to_owned()),
        };
    }
    ReflexOut::Refused(
        "reflex returned a map without goto or hold; expected exactly one".to_owned(),
    )
}

/// The cell a `goto` names, if it names one at all.
fn cell_of(value: &Dynamic) -> Option<CellXY> {
    let array = value.as_array_ref().ok()?;
    if array.len() != 2 {
        return None;
    }
    let x = u16::try_from(array[0].as_int().ok()?).ok()?;
    let y = u16::try_from(array[1].as_int().ok()?).ok()?;
    Some(CellXY { x, y })
}

/// The map a reflex sees: the score view, with sorted keys.
///
/// The keys are `beacons`, `enemies`, `me`, `score`, `squad` and `tick`,
/// in that order, because a Rhai map is a `BTreeMap`. Cells are `[x, y]`
/// arrays; a beacon's holder is the squad holding it, or [`HOLDER_NONE`].
fn view_map(view: &ScoreView) -> Dynamic {
    let mut map = Map::new();
    map.insert("beacons".into(), Dynamic::from(beacons(view)));
    map.insert("enemies".into(), Dynamic::from(enemies(view)));
    map.insert("me".into(), agent_map(&view.me, view.squad_id));
    map.insert("score".into(), Dynamic::from(score(view)));
    map.insert("squad".into(), Dynamic::from(squad(view)));
    map.insert("tick".into(), Dynamic::from(tick(view.tick)));
    Dynamic::from_map(map)
}

/// One agent as a reflex sees it.
fn agent_map(agent: &AgentSnap, squad: u8) -> Dynamic {
    let mut map = Map::new();
    map.insert("cell".into(), cell_array(agent.cell));
    map.insert("id".into(), Dynamic::from(i64::from(agent.id)));
    map.insert("role".into(), Dynamic::from(role_name(agent.role)));
    map.insert("squad".into(), Dynamic::from(i64::from(squad)));
    map.insert("stunned".into(), Dynamic::from(agent.stunned));
    Dynamic::from_map(map)
}

/// A cell as a reflex sees it: `[x, y]`.
fn cell_array(cell: CellXY) -> Dynamic {
    Dynamic::from(vec![
        Dynamic::from(i64::from(cell.x)),
        Dynamic::from(i64::from(cell.y)),
    ])
}

/// The enemies the squad remembers, as reflexes see them.
fn enemies(view: &ScoreView) -> Vec<Dynamic> {
    view.enemies
        .iter()
        .map(|e| {
            let mut map = Map::new();
            map.insert("age_ticks".into(), Dynamic::from(i64::from(e.age_ticks)));
            map.insert("cell".into(), cell_array(e.cell));
            map.insert("id".into(), Dynamic::from(i64::from(e.id)));
            Dynamic::from_map(map)
        })
        .collect()
}

/// The beacons of the round, as reflexes see them.
fn beacons(view: &ScoreView) -> Vec<Dynamic> {
    view.beacons
        .iter()
        .map(|b| {
            let holder = b.holder.map_or(HOLDER_NONE, i64::from);
            let mut map = Map::new();
            map.insert("cell".into(), cell_array(b.cell));
            map.insert("holder".into(), Dynamic::from(holder));
            Dynamic::from_map(map)
        })
        .collect()
}

/// The score of every squad, as reflexes see it.
fn score(view: &ScoreView) -> Vec<Dynamic> {
    view.score.iter().map(|s| Dynamic::from(*s)).collect()
}

/// The squad, as reflexes see it.
fn squad(view: &ScoreView) -> Vec<Dynamic> {
    view.squad
        .iter()
        .map(|a| agent_map(a, view.squad_id))
        .collect()
}

/// The tick, as reflexes see it.
fn tick(tick: u64) -> i64 {
    i64::try_from(tick).unwrap_or(i64::MAX)
}

/// The name of a role, as reflexes see it.
fn role_name(role: Role) -> &'static str {
    match role {
        Role::Leader => "leader",
        Role::Member => "member",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::{BeaconSnap, EnemySeen};

    /// The shipped reflex, compiled into the test binary.
    const DEFAULT_SRC: &str = include_str!("../../../assets/reflexes/default.rhai");

    /// A cell of the navigation grid.
    fn cell(x: u16, y: u16) -> CellXY {
        CellXY { x, y }
    }

    /// An agent snapshot.
    fn snap(id: u16, at: CellXY, role: Role) -> AgentSnap {
        AgentSnap {
            id,
            cell: at,
            role,
            stunned: false,
        }
    }

    /// A score view: one agent of squad 0, the given beacons and enemies.
    fn view(me: CellXY, beacons: &[(CellXY, Option<u8>)], enemies: &[(CellXY, u32)]) -> ScoreView {
        ScoreView {
            tick: 12,
            me: snap(0, me, Role::Member),
            squad: vec![snap(0, me, Role::Member)],
            enemies: enemies
                .iter()
                .map(|(at, age)| EnemySeen {
                    id: 9,
                    cell: *at,
                    age_ticks: *age,
                })
                .collect(),
            beacons: beacons
                .iter()
                .map(|(at, holder)| BeaconSnap {
                    cell: *at,
                    holder: *holder,
                })
                .collect(),
            score: vec![0, 0],
            squad_id: 0,
        }
    }

    /// A runtime with the shipped reflex loaded.
    fn with_default() -> RhaiRuntime {
        let mut rt = RhaiRuntime::new();
        rt.load("default", DEFAULT_SRC)
            .expect("the shipped reflex loads");
        rt
    }

    #[test]
    fn default_reflex_heads_for_free_beacon() {
        let mut rt = with_default();
        let view = view(cell(1, 1), &[(cell(5, 5), None)], &[]);
        assert_eq!(
            rt.reflex("default", &view, 10_000),
            ReflexOut::Cmd(MotorCmd::GoTo(cell(5, 5)))
        );
    }

    #[test]
    fn infinite_loop_runs_out_of_fuel() {
        let mut rt = RhaiRuntime::new();
        rt.load("loop", "fn reflex(v){ loop {} }")
            .expect("the script parses");
        let view = view(cell(1, 1), &[(cell(5, 5), None)], &[]);

        // The script never returns on its own: the fuel ends it.
        assert_eq!(rt.reflex("loop", &view, 1_000), ReflexOut::OutOfFuel);

        // The budget belongs to that call alone: the runtime still answers.
        rt.load("quiet", "fn reflex(v){ #{ hold: true } }")
            .expect("the script parses");
        assert_eq!(
            rt.reflex("quiet", &view, 1_000),
            ReflexOut::Cmd(MotorCmd::Hold)
        );
    }

    #[test]
    fn runtime_error_is_contained() {
        let mut rt = RhaiRuntime::new();
        rt.load("boom", "fn reflex(v){ v.nope.x }")
            .expect("the script parses");
        let view = view(cell(1, 1), &[(cell(5, 5), None)], &[]);
        assert!(matches!(
            rt.reflex("boom", &view, 10_000),
            ReflexOut::Error(_)
        ));
    }

    #[test]
    fn garbage_return_is_refused() {
        let mut rt = RhaiRuntime::new();
        let view = view(cell(1, 1), &[(cell(5, 5), None)], &[]);

        rt.load("number", "fn reflex(v){ 42 }")
            .expect("the script parses");
        assert!(matches!(
            rt.reflex("number", &view, 10_000),
            ReflexOut::Refused(_)
        ));

        rt.load("string", "fn reflex(v){ #{ \"goto\": \"x\" } }")
            .expect("the script parses");
        assert!(matches!(
            rt.reflex("string", &view, 10_000),
            ReflexOut::Refused(_)
        ));

        // Two answers at once are no answer at all.
        rt.load("both", "fn reflex(v){ #{ \"goto\": [2, 2], hold: true } }")
            .expect("the script parses");
        assert!(matches!(
            rt.reflex("both", &view, 10_000),
            ReflexOut::Refused(_)
        ));

        // A key the contract does not name is refused too.
        rt.load(
            "extra",
            "fn reflex(v){ #{ \"goto\": [2, 2], \"speed\": 3 } }",
        )
        .expect("the script parses");
        assert!(matches!(
            rt.reflex("extra", &view, 10_000),
            ReflexOut::Refused(_)
        ));

        // A cell that is not a cell is refused.
        rt.load("short", "fn reflex(v){ #{ \"goto\": [2] } }")
            .expect("the script parses");
        assert!(matches!(
            rt.reflex("short", &view, 10_000),
            ReflexOut::Refused(_)
        ));

        // A cell off the map cannot be addressed.
        rt.load("negative", "fn reflex(v){ #{ \"goto\": [-1, 2] } }")
            .expect("the script parses");
        assert!(matches!(
            rt.reflex("negative", &view, 10_000),
            ReflexOut::Refused(_)
        ));
    }

    #[test]
    fn parse_error_fails_round_setup() {
        let mut rt = RhaiRuntime::new();
        let err = rt
            .load("bad", "fn (")
            .expect_err("a script that does not parse must be refused");
        assert!(matches!(err, ScriptError::Parse(_)));
        // The broken script is not registered: the round cannot run it.
        let view = view(cell(1, 1), &[(cell(5, 5), None)], &[]);
        assert!(matches!(
            rt.reflex("bad", &view, 10_000),
            ReflexOut::Refused(_)
        ));
    }

    #[test]
    fn floats_unavailable() {
        let mut rt = RhaiRuntime::new();
        let err = rt
            .load("f", "fn reflex(v){ 1.5 }")
            .expect_err("a float literal must be refused");
        assert!(matches!(err, ScriptError::Parse(_)));
    }

    #[test]
    fn same_input_same_output() {
        let mut rt = with_default();
        let view = view(
            cell(2, 3),
            &[(cell(7, 1), None), (cell(4, 4), Some(1))],
            &[],
        );
        let first = rt.reflex("default", &view, 10_000);
        for _ in 0..100 {
            assert_eq!(rt.reflex("default", &view, 10_000), first);
        }
    }

    #[test]
    fn default_reflex_flees_a_near_enemy() {
        let mut rt = with_default();
        // The enemy is 2 cells west: the reflex steps 4 cells east.
        let view = view(cell(5, 5), &[(cell(9, 9), None)], &[(cell(3, 5), 0)]);
        assert_eq!(
            rt.reflex("default", &view, 10_000),
            ReflexOut::Cmd(MotorCmd::GoTo(cell(9, 5)))
        );
    }

    #[test]
    fn default_reflex_ignores_a_distant_enemy() {
        let mut rt = with_default();
        // The enemy is 7 cells away: too far to flee, so the beacon wins.
        let view = view(cell(5, 5), &[(cell(9, 9), None)], &[(cell(5, 12), 0)]);
        assert_eq!(
            rt.reflex("default", &view, 10_000),
            ReflexOut::Cmd(MotorCmd::GoTo(cell(9, 9)))
        );
    }

    #[test]
    fn default_reflex_clamps_the_flee_to_the_map() {
        let mut rt = with_default();
        // The enemy is east: the flee would leave the map, so it is clamped
        // to the origin of the grid.
        let view = view(cell(1, 1), &[(cell(9, 9), None)], &[(cell(5, 1), 0)]);
        assert_eq!(
            rt.reflex("default", &view, 10_000),
            ReflexOut::Cmd(MotorCmd::GoTo(cell(0, 1)))
        );
    }

    #[test]
    fn default_reflex_takes_the_nearest_free_beacon() {
        let mut rt = with_default();
        // Beacon 0 is ours, so it is skipped; beacon 1 is free and wins
        // even though it is further away.
        let view = view(
            cell(1, 1),
            &[(cell(2, 1), Some(0)), (cell(8, 1), None)],
            &[],
        );
        assert_eq!(
            rt.reflex("default", &view, 10_000),
            ReflexOut::Cmd(MotorCmd::GoTo(cell(8, 1)))
        );
    }

    #[test]
    fn default_reflex_contests_a_beacon_the_other_squad_holds() {
        let mut rt = with_default();
        // A beacon the other squad holds is still worth taking: the rule is
        // "the nearest beacon whose holder is not my squad".
        let view = view(cell(1, 1), &[(cell(2, 1), Some(1))], &[]);
        assert_eq!(
            rt.reflex("default", &view, 10_000),
            ReflexOut::Cmd(MotorCmd::GoTo(cell(2, 1)))
        );
    }

    #[test]
    fn default_reflex_holds_when_every_beacon_is_ours() {
        let mut rt = with_default();
        let view = view(cell(1, 1), &[(cell(5, 5), Some(0))], &[]);
        assert_eq!(
            rt.reflex("default", &view, 10_000),
            ReflexOut::Cmd(MotorCmd::Hold)
        );
    }

    #[test]
    fn view_map_has_the_contract_keys() {
        let view = view(cell(1, 1), &[(cell(5, 5), None)], &[(cell(2, 2), 3)]);
        let map = view_map(&view).try_cast::<Map>().expect("a map");
        let keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
        assert_eq!(keys, ["beacons", "enemies", "me", "score", "squad", "tick"]);

        // The cell of an agent is an [x, y] pair, and a free beacon says so.
        let me = map["me"].clone().try_cast::<Map>().expect("a map");
        let cell = me["cell"]
            .clone()
            .try_cast::<Vec<Dynamic>>()
            .expect("an array");
        assert_eq!(cell[0].as_int().ok(), Some(1));
        assert_eq!(cell[1].as_int().ok(), Some(1));
        let beacons = map["beacons"]
            .clone()
            .try_cast::<Vec<Dynamic>>()
            .expect("an array");
        let beacon = beacons[0].clone().try_cast::<Map>().expect("a map");
        assert_eq!(beacon["holder"].as_int().ok(), Some(HOLDER_NONE));
        assert_eq!(map["tick"].as_int().ok(), Some(12));
    }

    #[test]
    fn unknown_script_is_refused() {
        let mut rt = RhaiRuntime::new();
        let view = view(cell(1, 1), &[(cell(5, 5), None)], &[]);
        assert!(matches!(
            rt.reflex("missing", &view, 10_000),
            ReflexOut::Refused(_)
        ));
    }

    #[test]
    fn script_without_an_entry_point_is_refused() {
        let mut rt = RhaiRuntime::new();
        let err = rt
            .load("other", "fn other(v){ v }")
            .expect_err("a script without fn reflex(v) must be refused");
        assert!(matches!(err, ScriptError::NoEntryPoint(_)));
    }

    #[test]
    fn nothing_is_an_answer_of_its_own() {
        let mut rt = RhaiRuntime::new();
        rt.load("quiet", "fn reflex(v){ }")
            .expect("the script parses");
        let view = view(cell(1, 1), &[(cell(5, 5), None)], &[]);
        assert_eq!(rt.reflex("quiet", &view, 10_000), ReflexOut::Nothing);
    }
}
