//! The 3D scene model: the pure half of the Bevy view (phase 6 Task 6).
//!
//! The Bevy side of the 3D view is a dumb mapper, and everything it maps
//! *from* lives here, as plain data with no window and no GPU: which chunks
//! the view draws, where every agent's figure stands, which colour it wears,
//! and whether the round may start yet. A decision that can be made in plain
//! data is a decision that can be tested in WSL — and a decision the render
//! thread never has to make twice.
//!
//! The model reads the same sim the 2D map reads, the same way: positions
//! come from `lm-core`'s fixed-point `Pos`, are lerped between the previous
//! tick and the last one by the same overstep fraction
//! [`lm_host_model::map::MapModel::build`] uses, and become metres once, at
//! this boundary (D8). Any disagreement between the 2D map and the 3D view
//! is a bug in the 3D view.

use lm_core::fixed::{Angle, Pos, ANGLE_TURN, UNITS_PER_M};
use lm_core::sim::Sim;
use lm_core::world::World;

use crate::greedy::cells_per_chunk;
use crate::hash::{ChunkCache, Handle};
use crate::mesh::{ChunkCoord, ChunkMesh};
use crate::palette::{Color, MaterialClass, Palette, RenderConfig};

/// The chunks the 3D view draws: every chunk within `view_distance_chunks`
/// of the focus, in the world's own chunk grid.
///
/// The set is the *world's*, never the screen's: the focus moves with the
/// camera, but a chunk's identity is its world tile (D46). The chunks come
/// ordered nearest first (the focus chunk, then ring by ring), clipped by
/// the world's own extents and never larger than `max_chunks`.
pub fn visible_chunks(world: &World, focus_m: (f32, f32), cfg: &RenderConfig) -> Vec<ChunkCoord> {
    let cells = cells_per_chunk(world).max(1);
    let steps = i32::try_from(cells).unwrap_or(1);

    // The world's chunk grid, and the chunk the focus sits in, clamped into
    // it: a focus outside the world (a camera looking from beyond its edge)
    // still draws the nearest chunks, never none.
    let chunks_w = u32::from(world.grid.w).div_ceil(cells);
    let chunks_h = u32::from(world.grid.h).div_ceil(cells);
    let cell_m = world.cell_units as f32 / UNITS_PER_M as f32;
    let fx = ((focus_m.0 / cell_m).floor() as i32 / steps).clamp(0, chunks_w as i32 - 1);
    let fy = ((focus_m.1 / cell_m).floor() as i32 / steps).clamp(0, chunks_h as i32 - 1);

    // The largest ring that can still touch the world: beyond it every ring
    // is empty, whatever the view distance says.
    let last_useful = *[fx, fy, chunks_w as i32 - 1 - fx, chunks_h as i32 - 1 - fy]
        .iter()
        .max()
        .unwrap_or(&0);
    let distance = i32::try_from(cfg.view_distance_chunks).unwrap_or(0).max(0);

    let budget = cfg.max_chunks as usize;
    let mut out: Vec<ChunkCoord> = Vec::new();
    let push = |cx: i32, cy: i32, out: &mut Vec<ChunkCoord>| {
        let chunk = match (u8::try_from(cx), u8::try_from(cy)) {
            (Ok(bx), Ok(by))
                if (0..chunks_w as i32).contains(&cx) && (0..chunks_h as i32).contains(&cy) =>
            {
                ChunkCoord { cx: bx, cy: by }
            }
            _ => return,
        };
        out.push(chunk);
    };
    for d in 0..=distance.min(last_useful) {
        // The ring at Chebyshev distance d, in row-major order.
        for cy in (fy - d)..=(fy + d) {
            for cx in (fx - d)..=(fx + d) {
                if (cx - fx).abs() == d || (cy - fy).abs() == d {
                    push(cx, cy, &mut out);
                }
            }
        }
        if out.len() >= budget {
            break;
        }
    }
    out.truncate(budget);
    out
}

/// The chunk entities the view owns, as plain data: which world tile is
/// spawned, and the mesh handle behind it.
///
/// The Bevy side mirrors this set one entity per entry, and [`sync`] is the
/// only way it changes — so chunk entities are added and removed only when
/// the visible set itself changes, never per frame.
#[derive(Debug, Default)]
pub struct ChunkEntities {
    spawned: std::collections::BTreeMap<ChunkCoord, Handle<ChunkMesh>>,
}

impl ChunkEntities {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Bring the spawned set to `want`: the chunks it names are built (through
    /// `cache`, which is content-addressed) and added, the chunks it does not
    /// are dropped. Returns what changed, so the caller only spawns and
    /// despawns the difference.
    pub fn sync(
        &mut self,
        want: &[ChunkCoord],
        world: &World,
        cfg: &RenderConfig,
        palette: &Palette,
        cache: &mut ChunkCache,
    ) -> (Vec<(ChunkCoord, Handle<ChunkMesh>)>, Vec<ChunkCoord>) {
        let mut wanted = std::collections::BTreeSet::new();
        let mut added = Vec::new();
        for &chunk in want {
            if !wanted.insert(chunk) {
                continue; // the same chunk asked for twice is one chunk
            }
            if self.spawned.contains_key(&chunk) {
                continue; // already on screen: it is not spawned again
            }
            let handle = cache.get_or_build(world, chunk, cfg, palette);
            self.spawned.insert(chunk, handle);
            added.push((chunk, handle));
        }
        let removed: Vec<ChunkCoord> = self
            .spawned
            .keys()
            .filter(|chunk| !wanted.contains(*chunk))
            .copied()
            .collect();
        for chunk in &removed {
            self.spawned.remove(chunk);
        }
        (added, removed)
    }

    /// The spawned chunks and their mesh handles, in chunk order.
    pub fn chunks(&self) -> impl Iterator<Item = (&ChunkCoord, &Handle<ChunkMesh>)> {
        self.spawned.iter()
    }

    /// How many chunk entities the view owns.
    pub fn len(&self) -> usize {
        self.spawned.len()
    }

    /// Whether the view owns no chunk entity at all.
    pub fn is_empty(&self) -> bool {
        self.spawned.is_empty()
    }
}

/// One agent's figure as the 3D view draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FigureDraw {
    /// Which agent this is.
    pub id: u16,
    /// Which squad it belongs to.
    pub squad: u8,
    /// Where its feet stand, in metres: the position lerped between the
    /// previous tick and the last one by `alpha`.
    pub pos_m: (f32, f32),
    /// Which way it faces, in radians (a full turn is `2π`).
    pub facing_rad: f32,
    /// The colour of its material: the squad's own.
    pub color: Color,
}

/// A position lerped from `prev` to `cur` by `alpha`, in metres.
///
/// The same lerp the 2D map draws ([`lm_host_model::map::pos_m`]): it runs in
/// `f64` over the unit counts — the grid is powers of two, and its exact
/// midpoints must stay exact — and only the result becomes `f32`. Floats
/// exist here to be drawn, never to be measured.
pub fn pos_m(prev: Pos, cur: Pos, alpha: f32) -> (f32, f32) {
    let (px, py) = prev.to_units();
    let (cx, cy) = cur.to_units();
    let a = f64::from(alpha);
    let m = |from: i64, to: i64| {
        let units = (from as f64) + ((to as f64) - (from as f64)) * a;
        (units / f64::from(UNITS_PER_M)) as f32
    };
    (m(px, cx), m(py, cy))
}

/// An angle as radians: core counts 1/65536 of a turn, the renderer wants
/// the turn itself — the same conversion the 2D map's model makes.
fn facing_rad(facing: Angle) -> f32 {
    (f64::from(facing.0) * std::f64::consts::TAU / f64::from(ANGLE_TURN)) as f32
}

/// The figures of the round: one per agent of the roster, no more, in
/// agent-id order.
///
/// `prev` holds where every agent stood before the last tick, exactly as the
/// 2D map's model takes it; `alpha` is the fixed-timestep overstep fraction,
/// with 0 drawing the previous tick and 1 the last one. An agent with no
/// previous position (the first frame) draws where it stands.
pub fn agent_figures(
    sim: &Sim,
    prev: &[(u16, Pos)],
    alpha: f32,
    palette: &Palette,
) -> Vec<FigureDraw> {
    let mut roster = sim.agent_ids();
    roster.sort_unstable();
    roster.dedup();
    roster
        .iter()
        .filter_map(|&id| {
            let agent = sim.agent(id)?;
            let from = prev
                .iter()
                .find(|&&(seen, _)| seen == id)
                .map_or(agent.pos, |&(_, pos)| pos);
            Some(FigureDraw {
                id: agent.id,
                squad: agent.squad,
                pos_m: pos_m(from, agent.pos, alpha),
                facing_rad: facing_rad(agent.facing),
                color: palette.squad_color(agent.squad),
            })
        })
        .collect()
}

/// One beacon as the 3D view draws it (Task 8's reviewer finding 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeaconDraw {
    /// The centre of the beacon's cell, in metres. A beacon never moves, so
    /// it does not interpolate.
    pub pos_m: (f32, f32),
    /// The squad holding it, if any.
    pub holder: Option<u8>,
}

/// The round's beacons, in world order, with their holders (Task 8's
/// reviewer finding 4).
///
/// The beacons are pinned in the header — symmetric for both squads — so
/// they are the round's common knowledge: every viewer draws every beacon,
/// exactly as the 2D map does. The holders come from a view like the map's
/// own (any view; they are squad-level facts), and a round with no agents
/// has no view to ask and draws no beacons.
pub fn beacons(sim: &Sim) -> Vec<BeaconDraw> {
    let world = sim.world();
    let Some(&asker) = sim.agent_ids().first() else {
        return Vec::new();
    };
    sim.view(asker)
        .beacons
        .iter()
        .map(|b| BeaconDraw {
            pos_m: cell_centre_m(world, b.cell),
            holder: b.holder,
        })
        .collect()
}

/// The centre of a cell, in metres — the same arithmetic the 2D map's
/// `BeaconDraw` uses.
fn cell_centre_m(world: &World, cell: lm_core::world::CellXY) -> (f32, f32) {
    let cell_m = world.cell_units as f32 / UNITS_PER_M as f32;
    (
        (f32::from(cell.x) + 0.5) * cell_m,
        (f32::from(cell.y) + 0.5) * cell_m,
    )
}

/// How far the opening camera sits from the squad it frames (Task 8's
/// reviewer finding 3): far enough back that every agent of the squad is in
/// frame with room to spare, and never so far that the figures stop reading
/// as figures.
///
/// The first Task 8 opening sat 100 m back, which pushed a corner-spawned
/// squad's four figures to a handful of pixels, put the beacons — the
/// round's middle-distance towers — above the top of the frame, and left
/// the camera so far beyond the world's edge that a third of the frame was
/// the void outside it. A close overview frames the squad *and* what it
/// plays toward: the figures read at a glance, the beacons stand over the
/// horizon behind them, and the world fills the frame from its near edge.
pub fn overview_distance(squad_radius_m: f32) -> f32 {
    const MIN_DISTANCE_M: f32 = 20.0;
    // At the overview pitch (about 43°) and a 45° vertical field of view,
    // the frame's ground extent is roughly the distance itself; twice the
    // squad's radius plus a margin puts the whole squad comfortably inside.
    (MIN_DISTANCE_M).max(squad_radius_m * 2.4 + 12.0)
}

/// The yaw of the opening camera: which side of the squad it sits on.
///
/// The camera goes on the far side of the squad from the thing the round is
/// about — the **nearest beacon**, so the opening frame is the squad in the
/// foreground and the beacon it plays toward behind it — or, in a round with
/// no beacons, the far side from the world's centre, so the camera looks
/// into the world rather than across a corner into the void.
///
/// The yaw is the rig's own convention (see the host's `rig_transform`):
/// the camera's scene position is `focus + (sin yaw, -cos yaw) · ground`,
/// so `yaw 0` sits north of the focus and `atan2(dx, -dy)` sits it in the
/// scene direction `(dx, dy)`.
pub fn opening_yaw(focus_m: (f32, f32), beacons: &[BeaconDraw], world_m: (f32, f32)) -> f32 {
    let away = match beacons.iter().min_by(|a, b| {
        let da = (a.pos_m.0 - focus_m.0).mul_add(a.pos_m.0 - focus_m.0, 0.0)
            + (a.pos_m.1 - focus_m.1) * (a.pos_m.1 - focus_m.1);
        let db = (b.pos_m.0 - focus_m.0).mul_add(b.pos_m.0 - focus_m.0, 0.0)
            + (b.pos_m.1 - focus_m.1) * (b.pos_m.1 - focus_m.1);
        da.total_cmp(&db)
    }) {
        Some(nearest) => (focus_m.0 - nearest.pos_m.0, focus_m.1 - nearest.pos_m.1),
        // No beacons: away from the world's centre. A focus at the centre
        // itself has no away, and the yaw stays 0 (north) — any side is as
        // good as any other there.
        None => (focus_m.0 - world_m.0 * 0.5, focus_m.1 - world_m.1 * 0.5),
    };
    if away == (0.0, 0.0) {
        return 0.0;
    }
    f32::atan2(away.0, -away.1)
}

/// The radius of a squad: how far its furthest agent stands from the
/// squad's middle, in metres. An empty squad has no radius.
pub fn squad_radius_m(sim: &Sim, squad: u8) -> Option<f32> {
    let positions: Vec<(i64, i64)> = sim
        .agent_ids()
        .iter()
        .filter_map(|id| sim.agent(*id))
        .filter(|agent| agent.squad == squad)
        .map(|agent| agent.pos.to_units())
        .collect();
    let count = positions.len();
    if count == 0 {
        return None;
    }
    let sum = positions
        .iter()
        .fold((0i64, 0i64), |acc, p| (acc.0 + p.0, acc.1 + p.1));
    let middle = (sum.0 / count as i64, sum.1 / count as i64);
    let radius = positions
        .iter()
        .map(|p| {
            let (dx, dy) = (p.0 - middle.0, p.1 - middle.1);
            (((dx * dx + dy * dy) as f64).sqrt() / f64::from(UNITS_PER_M)) as f32
        })
        .fold(0.0f32, f32::max);
    Some(radius)
}

/// A material the 3D view warms before the round starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WarmupMaterial {
    /// A material class of the palette.
    Class(MaterialClass),
    /// The figure material of a squad.
    Squad(u8),
}

/// Every material the 3D view draws with: every class, every squad.
///
/// The set is fixed — nothing is generated per round, nothing is compiled
/// mid-round (the `night-street` lesson) — so warming exactly this set
/// before the round starts is warming all of it.
pub fn warmup_materials() -> Vec<WarmupMaterial> {
    let classes = [
        MaterialClass::Floor,
        MaterialClass::Wall,
        MaterialClass::Cover,
        MaterialClass::BeaconHeld,
        MaterialClass::BeaconFree,
    ]
    .into_iter()
    .map(WarmupMaterial::Class);
    let squads = (0u8..4).map(WarmupMaterial::Squad);
    classes.chain(squads).collect()
}

/// The warmup: which materials have been compiled, and whether the round may
/// start.
///
/// The view's materials are fixed, so the set it must warm is known upfront;
/// the host marks each material compiled once the warmup card has carried it
/// through a rendered frame, and the round does not start until every one of
/// them is compiled.
#[derive(Debug)]
pub struct Warmup {
    expected: std::collections::BTreeSet<WarmupMaterial>,
    compiled: std::collections::BTreeSet<WarmupMaterial>,
}

impl Default for Warmup {
    /// A warmup that expects every material the view draws with — the only
    /// warmup there is.
    fn default() -> Self {
        Self::new()
    }
}

impl Warmup {
    /// A warmup that expects every material the view draws with.
    pub fn new() -> Self {
        Warmup {
            expected: warmup_materials().into_iter().collect(),
            compiled: std::collections::BTreeSet::new(),
        }
    }

    /// The materials the warmup expects, in canonical order.
    pub fn expected(&self) -> impl Iterator<Item = WarmupMaterial> + '_ {
        self.expected.iter().copied()
    }

    /// Mark a material compiled.
    pub fn mark_compiled(&mut self, material: WarmupMaterial) {
        self.compiled.insert(material);
    }

    /// Mark every expected material compiled.
    pub fn mark_all_compiled(&mut self) {
        let expected: Vec<WarmupMaterial> = self.expected.iter().copied().collect();
        for material in expected {
            self.mark_compiled(material);
        }
    }

    /// Whether a material has been marked compiled.
    pub fn is_compiled(&self, material: WarmupMaterial) -> bool {
        self.compiled.contains(&material)
    }

    /// The expected materials that are not compiled yet.
    pub fn missing(&self) -> Vec<WarmupMaterial> {
        self.expected.difference(&self.compiled).copied().collect()
    }

    /// Whether the round may start: every expected material is compiled.
    pub fn ready(&self) -> bool {
        self.expected.is_subset(&self.compiled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lm_core::fixed::UNITS_PER_M;
    use lm_core::header::Header;
    use lm_core::params::Params;
    use lm_core::world::generate;

    /// The reflex every agent of the test round runs.
    const REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

    /// The round the 3D view watches: the standard one, unplayed.
    fn round() -> Sim {
        Sim::new(&Header::default_round(1, REFLEX)).expect("the round starts")
    }

    /// A render configuration with a view distance and a mesh budget.
    fn config(view_distance_chunks: u32, max_chunks: u32) -> RenderConfig {
        RenderConfig {
            view_distance_chunks,
            max_chunks,
            wall_height_m: 2.6,
            cover_height_m: 1.1,
            greedy: true,
            ..RenderConfig::default()
        }
    }

    /// Two metres, in the world's own units.
    const TWO_M_UNITS: i64 = 2048;

    #[test]
    fn chunk_entities_match_visible_chunks() {
        // The default world: 128x128 cells of 2 m, so 8x8 chunks of 32 m.
        let world = generate(1, &Params::default());
        let cfg = config(3, 64);
        let palette = Palette::default();

        // The focus at the world's centre: chunk (4, 4), and the ring of
        // chunks within 3 of it — the whole 7x7 ring is inside the world.
        let want = visible_chunks(&world, (128.0, 128.0), &cfg);
        assert_eq!(want.len(), 49, "a 7x7 ring of chunks");
        assert_eq!(
            want[0],
            ChunkCoord { cx: 4, cy: 4 },
            "the focus chunk first"
        );

        // The entities the view owns are exactly the visible chunks — the
        // set, not a superset and not a subset.
        let mut entities = ChunkEntities::new();
        let mut cache = ChunkCache::new();
        let (added, removed) = entities.sync(&want, &world, &cfg, &palette, &mut cache);
        assert_eq!(added.len(), 49, "every visible chunk spawned");
        assert!(removed.is_empty());
        let spawned: std::collections::BTreeSet<ChunkCoord> =
            entities.chunks().map(|(chunk, _)| *chunk).collect();
        let visible: std::collections::BTreeSet<ChunkCoord> = want.iter().copied().collect();
        assert_eq!(spawned, visible, "the spawned set equals the visible set");

        // A second sync over the same view changes nothing: chunk entities
        // are not added and removed every frame.
        let (added, removed) = entities.sync(&want, &world, &cfg, &palette, &mut cache);
        assert!(added.is_empty(), "nothing is spawned twice");
        assert!(removed.is_empty(), "nothing is despawned");
        assert_eq!(entities.len(), 49);

        // A focus at the world's corner: the ring is clipped by the world,
        // never by anything else.
        let corner = visible_chunks(&world, (2.0, 2.0), &cfg);
        assert_eq!(corner.len(), 16, "a 4x4 ring clipped at the corner");
        assert!(corner.contains(&ChunkCoord { cx: 0, cy: 0 }));

        // A budget smaller than the ring: the nearest chunks come first, and
        // no more than the budget.
        let capped = visible_chunks(&world, (128.0, 128.0), &config(3, 10));
        assert_eq!(capped.len(), 10, "the mesh budget holds");
        assert_eq!(capped[0], ChunkCoord { cx: 4, cy: 4 });
    }

    #[test]
    fn agent_entities_match_roster() {
        let sim = round();
        let palette = Palette::default();

        let figures = agent_figures(&sim, &[], 0.0, &palette);

        // One figure per agent of the roster, in agent-id order, no more.
        let mut roster = sim.agent_ids();
        roster.sort_unstable();
        roster.dedup();
        let ids: Vec<u16> = figures.iter().map(|figure| figure.id).collect();
        assert_eq!(ids, roster, "one entity per agent, no more");
    }

    #[test]
    fn agent_transform_interpolates() {
        let sim = round();
        let palette = Palette::default();

        // Where every agent stood "before the last tick": two metres east of
        // where it stands now, so the halfway point is one metre east.
        let prev: Vec<(u16, Pos)> = sim
            .agent_ids()
            .iter()
            .map(|&id| {
                let (x, y) = sim.agent(id).expect("an agent of the round").pos.to_units();
                (id, Pos::from_units(x + TWO_M_UNITS, y))
            })
            .collect();
        let now: Vec<(u16, (f32, f32))> = sim
            .agent_ids()
            .iter()
            .map(|&id| {
                let agent = sim.agent(id).expect("an agent of the round");
                let (x, y) = agent.pos.to_units();
                (
                    id,
                    (x as f32 / UNITS_PER_M as f32, y as f32 / UNITS_PER_M as f32),
                )
            })
            .collect();

        // Halfway between the two ticks, the figure sits halfway between the
        // two positions — the same lerp the 2D map draws.
        let figures = agent_figures(&sim, &prev, 0.5, &palette);
        for figure in &figures {
            let (_, cur) = now
                .iter()
                .find(|(id, _)| *id == figure.id)
                .expect("the agent of the figure");
            let expected = (cur.0 + 1.0, cur.1);
            assert!(
                (figure.pos_m.0 - expected.0).abs() < 1e-4
                    && (figure.pos_m.1 - expected.1).abs() < 1e-4,
                "agent {} at ({}, {}), halfway would be {:?}",
                figure.id,
                figure.pos_m.0,
                figure.pos_m.1,
                expected
            );
        }

        // The ends of the interpolation are the ticks themselves.
        let at_prev = agent_figures(&sim, &prev, 0.0, &palette);
        let at_cur = agent_figures(&sim, &prev, 1.0, &palette);
        for (figure, before) in at_prev.iter().zip(figures.iter()) {
            let (_, cur) = now
                .iter()
                .find(|(id, _)| *id == figure.id)
                .expect("the agent of the figure");
            assert!(
                (figure.pos_m.0 - (cur.0 + 2.0)).abs() < 1e-4,
                "alpha 0 draws the previous tick"
            );
            assert_eq!(figure.id, before.id);
        }
        for figure in &at_cur {
            let (_, cur) = now
                .iter()
                .find(|(id, _)| *id == figure.id)
                .expect("the agent of the figure");
            assert!(
                (figure.pos_m.0 - cur.0).abs() < 1e-4,
                "alpha 1 draws the last tick"
            );
        }
    }

    #[test]
    fn agent_colour_is_the_squad_colour() {
        let sim = round();
        let palette = Palette::default();

        let figures = agent_figures(&sim, &[], 0.0, &palette);
        for figure in &figures {
            let squad = sim.agent(figure.id).expect("an agent of the round").squad;
            assert_eq!(
                figure.color,
                palette.squad_color(squad),
                "agent {} of squad {} wears its squad's colour",
                figure.id,
                squad
            );
        }
    }

    #[test]
    fn warmup_covers_every_material_class() {
        // The warmup set holds every class of the palette, and every squad.
        let materials = warmup_materials();
        for class in [
            MaterialClass::Floor,
            MaterialClass::Wall,
            MaterialClass::Cover,
            MaterialClass::BeaconHeld,
            MaterialClass::BeaconFree,
        ] {
            assert!(
                materials.contains(&WarmupMaterial::Class(class)),
                "the warmup covers {class:?}"
            );
        }
        for squad in 0u8..4 {
            assert!(
                materials.contains(&WarmupMaterial::Squad(squad)),
                "the warmup covers squad {squad}"
            );
        }

        // The round may not start until every one of them is compiled: a
        // warmup that misses one class is not ready, and the missing one is
        // named.
        let mut warmup = Warmup::new();
        assert!(!warmup.ready(), "nothing is compiled at the start");
        for material in warmup_materials() {
            if material != WarmupMaterial::Class(MaterialClass::BeaconFree) {
                warmup.mark_compiled(material);
            }
        }
        assert!(!warmup.ready(), "a class is still missing");
        assert_eq!(
            warmup.missing(),
            vec![WarmupMaterial::Class(MaterialClass::BeaconFree)],
            "the missing class is named"
        );
        warmup.mark_compiled(WarmupMaterial::Class(MaterialClass::BeaconFree));
        assert!(warmup.ready(), "every class compiled, the round may start");
        assert!(warmup.missing().is_empty());
    }

    #[test]
    fn opening_yaw_faces_the_nearest_beacon() {
        let sim = round();
        let beacons = beacons(&sim);
        assert!(!beacons.is_empty(), "the round has beacons to face");

        // The home squad spawns at a corner; the nearest beacon is deep in
        // the world. The opening camera sits on the far side of the squad
        // from that beacon, so the frame is the squad with the beacon it
        // plays toward behind it.
        let focus = (7.0, 7.0);
        let nearest = beacons
            .iter()
            .min_by(|a, b| {
                let da = (a.pos_m.0 - focus.0).powi(2) + (a.pos_m.1 - focus.1).powi(2);
                let db = (b.pos_m.0 - focus.0).powi(2) + (b.pos_m.1 - focus.1).powi(2);
                da.total_cmp(&db)
            })
            .expect("a nearest beacon");
        let yaw = opening_yaw(focus, &beacons, (256.0, 256.0));
        let ground = 30.0;
        let camera = (focus.0 + ground * yaw.sin(), focus.1 - ground * yaw.cos());
        let away = (camera.0 - focus.0, camera.1 - focus.1);
        let toward = (nearest.pos_m.0 - focus.0, nearest.pos_m.1 - focus.1);
        let dot = away.0 * toward.0 + away.1 * toward.1;
        assert!(
            dot < 0.0,
            "the camera sits behind the squad, away from the beacon (dot {dot})"
        );

        // The look direction reaches the beacon's half of the world: the
        // beacon is inside the frame's forward hemisphere, not behind the
        // camera's back.
        let forward = (-away.0, -away.1);
        let facing = forward.0 * toward.0 + forward.1 * toward.1;
        assert!(facing > 0.0, "the camera looks toward the beacon");
    }

    #[test]
    fn opening_yaw_without_beacons_looks_into_the_world() {
        // No beacons: the camera takes the far side of the focus from the
        // world's centre, so it looks into the world — never across a
        // corner into the void (the first seed-1 screenshot did).
        let focus = (20.0, 20.0);
        let world = (256.0, 256.0);
        let yaw = opening_yaw(focus, &[], world);
        let ground = 30.0;
        let camera = (focus.0 + ground * yaw.sin(), focus.1 - ground * yaw.cos());
        let centre = (world.0 * 0.5, world.1 * 0.5);
        let away = (camera.0 - focus.0, camera.1 - focus.1);
        let toward = (centre.0 - focus.0, centre.1 - focus.1);
        assert!(
            away.0 * toward.0 + away.1 * toward.1 < 0.0,
            "the camera sits beyond the focus from the centre"
        );

        // A focus at the world's centre has no away side; the yaw is the
        // convention's own zero (north), finite and sane.
        let mid = opening_yaw((128.0, 128.0), &[], world);
        assert_eq!(mid, 0.0);
        let _ = focus;
    }

    #[test]
    fn overview_distance_frames_the_squad_closely() {
        // A tight squad (the seed-1 home squad's radius is about 2.8 m) gets
        // the close overview, not a 100 m one: the figures have to read as
        // figures, and the beacons behind them have to stay under the top of
        // the frame. A spread-out squad still gets the distance it needs.
        assert_eq!(overview_distance(0.0), 20.0);
        assert_eq!(overview_distance(2.8), 20.0);
        assert!((overview_distance(30.0) - (30.0 * 2.4 + 12.0)).abs() < 1e-4);
    }
}
