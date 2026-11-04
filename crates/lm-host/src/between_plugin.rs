//! The between-round screen: what the flow decided, and what a human does about it.
//!
//! When a round is over, the learning flow runs: it plays the round again
//! headless, asks its reviewer what should change, seals the prediction of
//! what that change will do, tests it on held-out seeds and writes the chain
//! down. The flow is slow — a round is a round — so it runs on **its own
//! thread** and the frame never waits for it: the app polls for the answer
//! and shows the screen when it arrives.
//!
//! The screen itself is [`lm_host_model::between::BetweenModel`]: the metrics
//! with their change against the round before, the proposal, the sealed
//! prediction and the verdict, and the three buttons a human has. The two
//! calls a human can make go straight back into `lm-learn`:
//!
//! * **Adopt / Reject / Override** — the flow's decision, or a decision
//!   against it. An override is written into the ledger with the human's
//!   reason, and the measurement stays where it was (D32).
//! * **The request box** — what a human asks for. It becomes a deposit, and
//!   the deposit is what the reviewer is asked about next (D33).
//!
//! And **Next round** plays round N+1 with the header the flow wrote: the
//! harness pins and all, so the next round starts from what was learned
//! (D27).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Mutex;
use std::thread;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use lm_core::header::Header;
use lm_host_model::between::{metrics_of_path, Action, BetweenModel, Section};
use lm_host_model::host_loop::HostLoop;
use lm_host_model::record;
use lm_learn::flow::{run_learning_round, FlowConfig, RouterReviewer, StubReviewer};

use crate::sim_plugin::{Recording, Round};
use crate::AgentChoice;

/// Who reviews the round.
///
/// The stub answers the same thing every time, with no network and no key: a
/// dry run of the chain. The router asks the cloud pool, and spends money
/// doing it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewerChoice {
    /// The stub: no network, no key.
    Stub,
    /// The router: the cloud pool.
    Router,
}

impl ReviewerChoice {
    /// The choice a command line spells out.
    pub fn parse(text: &str) -> Result<ReviewerChoice, String> {
        match text {
            "stub" => Ok(ReviewerChoice::Stub),
            "router" => Ok(ReviewerChoice::Router),
            other => Err(format!("`--reviewer` wants stub or router, not `{other}`")),
        }
    }

    /// The choice, as it is written on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            ReviewerChoice::Stub => "stub",
            ReviewerChoice::Router => "router",
        }
    }
}

/// How the flow is run.
#[derive(Resource, Clone, Debug)]
pub struct FlowSettings {
    /// Where the harnesses are: one directory per agent (`a0`, `a1`, ...).
    pub harness_root: PathBuf,
    /// Where the flow writes: the record, the ledger and the deposits.
    pub runs_root: PathBuf,
    /// Who reviews.
    pub reviewer: ReviewerChoice,
    /// How many ticks the learning round plays.
    pub ticks: u64,
    /// The stamp that names this session's rounds, so two sessions never
    /// write over each other's records.
    pub stamp: u64,
}

/// Where the between-round screen is in its story.
#[derive(Resource, Default)]
pub enum Between {
    /// Nothing to show yet: the round is still playing.
    #[default]
    Idle,
    /// The flow is running on its own thread.
    Running,
    /// The flow answered: the screen is ready.
    Ready(BetweenModel),
    /// The flow could not run.
    Failed(String),
}

/// The flow's answer, when it comes.
///
/// The receiver sits behind a lock: a resource has to be shareable, and a
/// channel is not.
#[derive(Resource, Default)]
pub struct Inbox(pub Option<Mutex<Receiver<Result<BetweenModel, String>>>>);

/// The last round's log: the one the next round's deltas are against.
#[derive(Resource, Default)]
pub struct LastLog(pub Option<PathBuf>);

/// Whether the between-round window is open.
#[derive(Resource, Default)]
pub struct WindowOpen(pub bool);

/// What the human is typing.
#[derive(Resource)]
pub struct BetweenUi {
    /// Why they went against the measurement.
    pub reason: String,
    /// What they want changed.
    pub request: String,
    /// What went wrong, if something did.
    pub error: Option<String>,
    /// What happened, in one line.
    pub note: Option<String>,
}

impl Default for BetweenUi {
    /// The window opens by itself: the screen has something to say.
    fn default() -> BetweenUi {
        BetweenUi {
            reason: String::new(),
            request: String::new(),
            error: None,
            note: None,
        }
    }
}

/// What the next round needs to start.
#[derive(Resource, Clone, Debug)]
pub struct NextRound {
    /// Who is thinking.
    pub agents: AgentChoice,
    /// The agents' configuration, when the command line named one.
    pub config: Option<PathBuf>,
    /// The harness the agents think with, when the flow has none of its own.
    pub harness: PathBuf,
    /// The flow's harness root.
    pub harness_root: PathBuf,
}

impl NextRound {
    /// The harness the agents think with: the flow's own, once it has one.
    pub fn harness_dir(&self) -> PathBuf {
        agent_harness(&self.harness_root).unwrap_or_else(|| self.harness.clone())
    }
}

/// The between-round plugin: the flow, the window, and the next round.
pub struct BetweenPlugin {
    /// How the flow is run.
    settings: FlowSettings,
    /// What the next round needs to start.
    next: NextRound,
}

impl BetweenPlugin {
    /// The plugin, with the flow's settings and what the next round needs.
    pub fn new(settings: FlowSettings, next: NextRound) -> BetweenPlugin {
        BetweenPlugin { settings, next }
    }
}

impl Plugin for BetweenPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings.clone())
            .insert_resource(self.next.clone())
            .init_resource::<Between>()
            .init_resource::<Inbox>()
            .init_resource::<WindowOpen>()
            .init_resource::<LastLog>()
            .init_resource::<BetweenUi>()
            .add_systems(Update, start_flow.before(poll_flow))
            .add_systems(Update, poll_flow)
            .add_systems(EguiPrimaryContextPass, between_window);
    }
}

/// Start the flow when the round is over.
///
/// The flow plays its own round and asks its reviewer; it runs on its own
/// thread, so the frame never waits for it.
fn start_flow(
    mut between: ResMut<Between>,
    mut inbox: ResMut<Inbox>,
    last_log: Res<LastLog>,
    round: Res<Round>,
    recording: Res<Recording>,
    settings: Res<FlowSettings>,
) {
    if !matches!(*between, Between::Idle) {
        return;
    }
    // The round is over when it has been written down and the log it wrote is
    // the one on screen. A window that closes mid-round is not a round.
    if !recording.written {
        return;
    }
    let Some(header) = round.header() else {
        return;
    };
    let seed = header.seed;
    let previous = last_log.0.clone();
    let settings = settings.clone();
    let (tx, rx) = channel();
    *inbox = Inbox(Some(Mutex::new(rx)));
    *between = Between::Running;
    thread::spawn(move || {
        let answer = run_the_flow(&settings, seed, previous);
        let _ = tx.send(answer);
    });
}

/// Run the flow on this thread: the round, the review, the test, the chain.
///
/// The screen is built here too, so the frame only ever receives something
/// that is ready to be shown.
fn run_the_flow(
    settings: &FlowSettings,
    seed: u64,
    previous_log: Option<PathBuf>,
) -> Result<BetweenModel, String> {
    let cfg = FlowConfig::new(seed, settings.harness_root.clone())
        .with_runs_root(settings.runs_root.clone())
        .with_ticks(settings.ticks)
        .with_round_id(format!("r{seed}-{}", settings.stamp));

    let record = match settings.reviewer {
        ReviewerChoice::Stub => {
            let mut reviewer = StubReviewer::harmless();
            run_learning_round(&cfg, &mut reviewer)
        }
        ReviewerChoice::Router => {
            let mut reviewer = RouterReviewer::discover().map_err(|err| err.to_string())?;
            run_learning_round(&cfg, &mut reviewer)
        }
    }
    .map_err(|err| err.to_string())?;

    // The metrics of the round before, for the deltas.
    let previous = previous_log.as_deref().and_then(metrics_of_path);
    Ok(BetweenModel::measured(&record, previous.as_ref()))
}

/// Take the flow's answer when it comes.
///
/// Only while the flow is running: once the answer is in, the channel is
/// empty and closed, and that is not a failure.
fn poll_flow(
    mut between: ResMut<Between>,
    inbox: Res<Inbox>,
    mut last_log: ResMut<LastLog>,
    mut open: ResMut<WindowOpen>,
) {
    if !matches!(*between, Between::Running) {
        return;
    }
    let Some(rx) = inbox.0.as_ref() else {
        return;
    };
    let Ok(rx) = rx.lock() else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(model)) => {
            last_log.0 = Some(model.record().log_path.clone());
            open.0 = true;
            *between = Between::Ready(model);
        }
        Ok(Err(why)) => {
            open.0 = true;
            *between = Between::Failed(why);
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            open.0 = true;
            *between = Between::Failed("the flow stopped without an answer".to_string());
        }
    }
}

/// The between-round window.
fn between_window(
    mut contexts: EguiContexts,
    mut between: ResMut<Between>,
    mut state: ResMut<BetweenUi>,
    mut open: ResMut<WindowOpen>,
    mut round: ResMut<Round>,
    mut recording: ResMut<Recording>,
    setup: Res<NextRound>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();

    let model = match &*between {
        Between::Ready(model) => model.clone(),
        Between::Running => {
            egui::Window::new("between rounds")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label("the flow is running: the round is measured and the proposal tested");
                });
            return Ok(());
        }
        Between::Failed(why) => {
            let why = why.clone();
            egui::Window::new("between rounds")
                .open(&mut open.0)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 120, 110),
                        format!("the flow did not run: {why}"),
                    );
                });
            return Ok(());
        }
        Between::Idle => return Ok(()),
    };

    // What the human pressed, and what they asked for.
    let mut pressed: Option<Action> = None;
    let mut asked = false;
    let mut next = false;

    egui::Window::new(format!("between rounds — {}", model.round_id))
        .open(&mut open.0)
        .default_width(560.0)
        .show(&ctx, |ui| {
            draw_screen(ui, &model, &mut state, &mut pressed, &mut asked, &mut next);
        });

    // A decision: adopt, reject, or go against the measurement.
    if let Some(action) = pressed {
        let reason = state.reason.clone();
        match model.apply(action, &reason) {
            Ok(record) => {
                *between = Between::Ready(model.replaced(&record));
                state.error = None;
                state.note = Some(match action {
                    Action::Adopt => "the change is adopted".to_string(),
                    Action::Reject => "the change is refused".to_string(),
                    Action::Override => "the decision is now the human's".to_string(),
                });
            }
            Err(err) => state.error = Some(err.to_string()),
        }
    }

    // A request: the human's words, kept verbatim, as a deposit.
    if asked {
        let text = state.request.clone();
        let mut next_model = model.clone();
        match next_model.write_request(&text) {
            Ok(path) => {
                *between = Between::Ready(next_model);
                state.request.clear();
                state.error = None;
                state.note = Some(format!("the request is in {}", path.display()));
            }
            Err(err) => state.error = Some(err.to_string()),
        }
    }

    // The next round: round N+1, with the header the flow wrote.
    if next {
        match start_next_round(&model, &mut round, &mut recording, &setup) {
            Ok(()) => {
                *between = Between::Idle;
                open.0 = false;
                state.reason.clear();
                state.note = Some("the next round is under way".to_string());
            }
            Err(err) => state.error = Some(err),
        }
    }

    Ok(())
}

/// The screen itself.
fn draw_screen(
    ui: &mut egui::Ui,
    model: &BetweenModel,
    state: &mut BetweenUi,
    pressed: &mut Option<Action>,
    asked: &mut bool,
    next: &mut bool,
) {
    ui.label(format!("round {} (seed {})", model.round_id, model.seed));

    // The screen is drawn in the order the model says: the promise before the
    // measurement that judges it.
    for section in model.sections() {
        match section {
            Section::Metrics => draw_metrics(ui, model),
            Section::Proposal => draw_proposal(ui, model),
            Section::Prediction => draw_prediction(ui, model),
            Section::Verdict => draw_verdict(ui, model),
            Section::Actions => draw_actions(ui, model, state, pressed),
        }
    }

    // The request box.
    ui.separator();
    ui.label("ask for a change:");
    ui.text_edit_multiline(&mut state.request);
    if ui.button("send the request").clicked() {
        *asked = true;
    }

    // The next round.
    ui.separator();
    if ui.button("next round").clicked() {
        *next = true;
    }
    ui.label(format!(
        "the next round pins {} ({}).",
        model.next_header_pin,
        model.next_header_path.display()
    ));

    // What happened, and what went wrong.
    if let Some(note) = &state.note {
        ui.colored_label(egui::Color32::from_rgb(160, 220, 160), note.clone());
    }
    if let Some(err) = &state.error {
        ui.colored_label(egui::Color32::from_rgb(255, 120, 110), err.clone());
    }
}

/// The metrics, and the change against the round before.
fn draw_metrics(ui: &mut egui::Ui, model: &BetweenModel) {
    if !model.metrics.is_empty() {
        ui.separator();
        ui.label("what the round measured");
        egui::Grid::new("between_metrics")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                ui.label("metric");
                ui.label("value");
                ui.label("change");
                ui.end_row();
                for line in &model.metrics {
                    ui.label(line.name.clone());
                    ui.label(match line.value {
                        Some(value) => value.to_string(),
                        None => "—".to_string(),
                    });
                    ui.label(match line.delta {
                        Some(delta) => format!("{delta:+}"),
                        None => "—".to_string(),
                    });
                    ui.end_row();
                }
            });
    }
}

/// The proposal, in words and in a diff.
fn draw_proposal(ui: &mut egui::Ui, model: &BetweenModel) {
    if let Some(proposal) = &model.proposal {
        ui.separator();
        ui.label("what was proposed");
        ui.label(proposal.summary.clone());
        if !proposal.diff.is_empty() {
            ui.monospace(truncate(&proposal.diff, 2_000));
        }
    }
    if let Some(why) = &model.rejection {
        ui.colored_label(
            egui::Color32::from_rgb(255, 170, 110),
            format!("refused: {why}"),
        );
    }
}

/// The prediction that was sealed before the test.
fn draw_prediction(ui: &mut egui::Ui, model: &BetweenModel) {
    if let Some(prediction) = &model.prediction {
        ui.separator();
        ui.label("what was promised, before the test");
        ui.label(format!(
            "seal {} — {} {} by at least {}, judged on seeds {}",
            prediction.seal,
            prediction.metric,
            match prediction.direction {
                lm_learn::ledger::Direction::Up => "rises",
                lm_learn::ledger::Direction::Down => "falls",
            },
            prediction.min_delta,
            prediction
                .eval_seeds
                .iter()
                .map(|seed| seed.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

/// The verdict, as it was measured.
fn draw_verdict(ui: &mut egui::Ui, model: &BetweenModel) {
    if let Some(verdict) = &model.verdict {
        ui.separator();
        ui.label("what the test measured");
        ui.label(format!(
            "{} — {} of {} seeds moved as promised",
            if verdict.adopted {
                "adopted"
            } else {
                "refused"
            },
            verdict.moved,
            verdict.seeds
        ));
        ui.label(
            verdict
                .deltas
                .iter()
                .map(|(seed, delta)| format!("{seed}:{delta:+}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
}

/// What the flow decided, and what a human may do about it.
fn draw_actions(
    ui: &mut egui::Ui,
    model: &BetweenModel,
    state: &mut BetweenUi,
    pressed: &mut Option<Action>,
) {
    ui.separator();
    ui.label(format!("the flow decided: {}", model.decision));
    ui.label("why (for a decision against the measurement):");
    ui.text_edit_singleline(&mut state.reason);
    ui.horizontal(|ui| {
        for action in model.actions.clone() {
            let label = action.as_str();
            if ui.button(label).clicked() {
                *pressed = Some(action);
            }
        }
    });
}

/// Start round N+1 with the header the flow wrote.
///
/// The header carries the harness pins, so the round that starts here plays
/// with what the flow learned (D27).
fn start_next_round(
    model: &BetweenModel,
    round: &mut Round,
    recording: &mut Recording,
    setup: &NextRound,
) -> Result<(), String> {
    let header: Header =
        lm_host_model::between::next_header(model.record()).map_err(|err| err.to_string())?;
    let mut host = HostLoop::new(header.clone()).map_err(|err| err.to_string())?;

    // The agents think again, with the harness the flow left behind.
    if setup.agents != AgentChoice::Off {
        let harness = setup.harness_dir();
        crate::attach_agents(
            setup.agents,
            setup.config.as_deref(),
            &harness,
            &header,
            &mut host,
        )?;
    }

    round.live = Some(host);
    round.replay = None;
    round.error = None;
    round.source = None;
    recording.path = Some(record::path_for(header.seed, crate::unix_secs()));
    recording.written = false;
    Ok(())
}

/// The first agent's harness under a root: `a0`, `a1`, ...
pub fn agent_harness(root: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| is_agent_dir(name))
        })
        .collect();
    dirs.sort();
    dirs.into_iter().next()
}

/// `a0` is an agent's harness; anything else is not.
fn is_agent_dir(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('a') else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

/// Make sure the flow has a harness to work on.
///
/// The flow wants one directory per agent (`a0`, `a1`, ...). When the root has
/// none, the host copies the round's own harness into `a0`, so the flow has
/// something to change -- and makes it a git repository of its own, so a
/// change that passes is committed like any other.
pub fn seed_harness_root(root: &Path, harness: &Path) -> io::Result<()> {
    if agent_harness(root).is_some() {
        return Ok(());
    }
    let a0 = root.join("a0");
    fs::create_dir_all(a0.join("reflexes"))?;
    let reflex = a0.join("reflexes/default.rhai");
    if !reflex.exists() {
        fs::write(&reflex, lm_tools::runner::DEFAULT_REFLEX)?;
    }
    for name in ["gates.toml", "policy.toml", "phrasebook.toml"] {
        let from = harness.join(name);
        let to = a0.join(name);
        if from.is_file() && !to.exists() {
            fs::copy(&from, &to)?;
        }
    }
    // A git repository of its own, so a change that passes is committed.
    if !a0.join(".git").exists() {
        let _ = git(&a0, &["init", "-q"]);
        let _ = git(&a0, &["add", "-A"]);
        let _ = git(
            &a0,
            &[
                "-c",
                "user.name=last-mile host",
                "-c",
                "user.email=host@last-mile.invalid",
                "commit",
                "-q",
                "-m",
                "the harness as it was",
            ],
        );
    }
    Ok(())
}

/// Run git in a directory, and say what it said.
fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|err| err.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

/// Cut a long text off, so one huge diff does not fill the window.
fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut out = text[..limit].to_string();
    out.push_str("\n… (cut off)");
    out
}
