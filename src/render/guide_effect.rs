//! Take-apart pictures for the battle effects guide: one effect kind at its
//! three variants beside the plan word that picks them, and the particle
//! pools of the Djinn attacks, the spark groups, the stat reductions and the
//! target strikes, each slot read from the effect's memory after every frame.

use super::battle::{self, Scene};
use super::field::Image;
use super::guide::Guide;
use super::guide_more::{canvas, mix, paste, screen};
use super::guide_take::text;
use super::mgba::{Gba, HEIGHT, WIDTH};
use super::plan::{self, Ally, Aim, Cast, Command, Tables};

/// gWorkSlot[HEAP_SLOT_BATTLE_EFFECT] holds struct BattleEffectWork
/// (battle_effect_work.h); its particles are 64 struct EffectStep from
/// 0x7080. PARTICLES is gMapGridCellBuffer, read as struct EffectStep.
const HEAP_SLOT_BATTLE_EFFECT: u32 = 39;
const FX_PARTICLES: u32 = 0x7080;
/// struct EffectStep: x, y, z, three velocities, then variant.
const STEP_SIZE: u32 = 28;
const STEP_VARIANT: u32 = 24;
/// gBattleWork->plan (struct BattlePlan) and its presentationFlags.
const WORK_PLAN: u32 = 0x654;
const PLAN_PRESENTATION_FLAGS: u32 = 0x58;
/// struct BattleEffectArgument's kind and variant.
const EFFECT_KIND: u32 = 0x00;
const EFFECT_VARIANT: u32 = 0x18;
const EFFECT_COUNT: u32 = 0x14;
const EFFECT_ACTORS: u32 = 0x24;
/// The first enemy unit id.
const FIRST_ENEMY_UNIT: u32 = 128;
/// BattlePlan.presentationFlags' kind and variant (STAGING_ in
/// battle_internal.h).
const STAGING_KIND_MASK: u32 = 0xfff;
const STAGING_VARIANT_MASK: u32 = 0x3000;
const STAGING_VARIANT_SHIFT: u32 = 12;

/// The effect kinds' names by value (enum BattleEffectKind) and the
/// variants' in order (enum EffectVariant), from effect.h.
fn effect_names() -> Result<(std::collections::HashMap<u32, String>, Vec<String>), String> {
    let path = crate::compile::root().join("games/tbs/include/unit/effect/effect.h");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut kinds = std::collections::HashMap::new();
    let mut variants = Vec::new();
    for line in text.lines() {
        let line = line.split("///").next().unwrap_or("").trim().trim_end_matches(',');
        if let Some((name, value)) = line.split_once(" = ") {
            if name.starts_with("BATTLE_EFFECT_") {
                if let Ok(v) = value.trim().parse() {
                    kinds.insert(v, name.to_string());
                }
            }
        } else if line.starts_with("EFFECT_VARIANT_") && !line.contains(' ') {
            variants.push(line.to_string());
        }
    }
    Ok((kinds, variants))
}

/// Frames kept before the effect's work block is taken and after it is
/// released.
const MARGIN: usize = 8;

const BACK: u32 = 0x20_22_28;
const TEXT: u32 = 0xe8_e8_e8;
const DIM: u32 = 0x80_84_8c;
const HEAD: u32 = 0x9c_c8_ff;
const MARK: u32 = 0xff_d8_40;
/// A slot the effect has not touched, one parked or waiting, and one done.
const FREE: u32 = 0x2a_2c_33;
const PARKED: u32 = 0x3c_4c_6c;
const SPENT: u32 = 0x44_40_3c;

/// Which pool a range of slots is in.
#[derive(Clone, Copy)]
enum Pool {
    Work,
    Particles,
}

/// A range of slots read each frame.
#[derive(Clone, Copy)]
struct Range {
    pool: Pool,
    first: u32,
    count: u32,
}

/// One slot: x, y, z and variant.
type Slot = [i32; 4];

/// One frame: the screen, whether the effect's work block is allocated, and
/// each range's slots.
struct Frame {
    screen: Vec<u16>,
    running: bool,
    ranges: Vec<Vec<Slot>>,
}

fn read_slots(gba: &Gba, base: u32, first: u32, count: u32) -> Vec<Slot> {
    (first..first + count)
        .map(|i| {
            let s = base + STEP_SIZE * i;
            [gba.read32(s) as i32, gba.read32(s + 4) as i32, gba.read32(s + 8) as i32, gba.read32(s + STEP_VARIANT) as i32]
        })
        .collect()
}

/// A slot drawn as a square of `cell` pixels, `cols` to a row.
fn grid(image: &mut Image, x: usize, y: usize, cols: usize, cell: usize, colours: &[u32]) -> usize {
    for (i, &c) in colours.iter().enumerate() {
        let (cx, cy) = (x + (i % cols) * cell, y + (i / cols) * cell);
        for py in cy..(cy + cell - 1).min(image.height) {
            for px in cx..(cx + cell - 1).min(image.width) {
                image.pixels[py * image.width + px] = c;
            }
        }
    }
    colours.len().div_ceil(cols) * cell
}

/// A swatch and its meaning, on one line; returns the width drawn.
fn key(image: &mut Image, x: i32, y: i32, colour: u32, label: &str) -> i32 {
    for py in 0..10 {
        for px in 0..10 {
            let (ix, iy) = ((x + px) as usize, (y + py) as usize);
            if ix < image.width && iy < image.height {
                image.pixels[iy * image.width + ix] = colour;
            }
        }
    }
    14 + text(image, x + 12, y - 2, label, TEXT, None, 2) + 10
}

/// A life count drawn from dim to bright.
fn life(v: i32, most: i32, colour: u32) -> u32 {
    mix(mix(colour, 0, 0.7), colour, (f64::from(v) / f64::from(most)).clamp(0.0, 1.0))
}

impl<'a> Guide<'a> {
    fn find_cast(&self, name: &str, summon: bool) -> Result<Cast, String> {
        let plan = Tables::read(crate::compile::root())?.plan()?;
        plan.casts
            .into_iter()
            .find(|c| c.ability.name == name && matches!(c.command, Command::Summon { .. }) == summon)
            .ok_or_else(|| format!("no {name} cast"))
    }

    /// An ability only enemies use, given to Isaac as a Psynergy command
    /// against a copy of Ivan: the command record carries its id, and the
    /// plan and effect follow its actflags.tsv row like any other.
    fn enemy_cast(&self, name: &str) -> Result<Cast, String> {
        let tables = Tables::read(crate::compile::root())?;
        let ability = plan::abilities(&tables.actions)?.into_iter().find(|a| a.name == name).ok_or_else(|| format!("no ability {name}"))?;
        let quake = self.find_cast("Quake", false)?;
        Ok(Cast { slug: plan::slug(name), ability, command: Command::Psynergy, preset: quake.preset, party: vec![plan::ISAAC], foes: vec![plan::IVAN], aim: Aim::Foe, ally: Ally::Healthy, lucky_foes: false })
    }

    /// Plays `cast` and reads `ranges` after every frame; `hooks` are
    /// function entries at which `at_hook` reads the machine. Returns the
    /// frames from shortly before the effect's work block is taken to
    /// shortly after it is released, and the index of the first running one.
    fn record_pools(&self, cast: &Cast, ranges: &[Range], hooks: &[u32], at_hook: &mut dyn FnMut(&Gba, u32, usize)) -> Result<(Vec<Frame>, usize), String> {
        let symbols = battle::Symbols::find(self.map)?;
        let work_slots = self.sym("gWorkSlot")?;
        let particles = self.sym("gMapGridCellBuffer")?;
        let mut frames: Vec<Frame> = Vec::new();
        battle::record_watching(self.rom, &symbols, cast, &Scene::Game, false, None, hooks, &mut |gba, hook| match hook {
            Some(pc) => at_hook(gba, pc, frames.len()),
            None => {
                let work = gba.read32(work_slots + 4 * HEAP_SLOT_BATTLE_EFFECT);
                let read = ranges
                    .iter()
                    .map(|r| match r.pool {
                        Pool::Work if work != 0 => read_slots(gba, work + FX_PARTICLES, r.first, r.count),
                        Pool::Work => vec![[0, 0, 0, 0]; r.count as usize],
                        Pool::Particles => read_slots(gba, particles, r.first, r.count),
                    })
                    .collect();
                frames.push(Frame { screen: gba.picture(), running: work != 0, ranges: read });
            }
        })?;
        let first = frames.iter().position(|f| f.running).ok_or("the effect never ran")?;
        let last = frames.iter().rposition(|f| f.running).unwrap_or(first);
        let (from, to) = (first.saturating_sub(MARGIN), (last + MARGIN).min(frames.len() - 1));
        Ok((frames.drain(from..=to).collect(), first - from))
    }

    /// Quake, Earthquake and Quake Sphere side by side, each from the frame
    /// its effect starts, as `effect-variants.gif`: above each, the
    /// presentationFlags word its plan holds and the kind and variant
    /// BattleEffect_RunQuake is handed, both read as the handler is entered.
    pub fn effect_variants(&mut self) -> Result<(), String> {
        let quake = self.sym("BattleEffect_RunQuake")?;
        let battle_work = self.sym("gBattleWork")?;
        let mut columns: Vec<(String, u32, u32, u32, Vec<Vec<u16>>)> = Vec::new();
        for name in ["Quake", "Earthquake", "Quake Sphere"] {
            let cast = self.find_cast(name, false)?;
            let mut read: Option<(u32, u32, u32, usize)> = None;
            let (frames, start) = self.record_pools(&cast, &[], &[quake], &mut |gba, _, at| {
                let plan = gba.read32(battle_work) + WORK_PLAN;
                let effect = gba.register(0);
                read = Some((gba.read32(plan + PLAN_PRESENTATION_FLAGS), gba.read32(effect + EFFECT_KIND), gba.read32(effect + EFFECT_VARIANT), at));
            })?;
            let (flags, kind, variant, _) = read.ok_or(format!("{name} never reached BattleEffect_RunQuake"))?;
            eprintln!("effect-variants: {name}: presentationFlags {flags:#x}, effect kind {kind}, variant {variant}; {} frames, effect from {start}", frames.len());
            columns.push((name.to_string(), flags, kind, variant, frames.into_iter().skip(start.saturating_sub(4)).map(|f| f.screen).collect()));
        }
        let (kinds, variants) = effect_names()?;
        let (top, gap) = (84, 12);
        let w = 3 * WIDTH + 4 * gap;
        let h = top + HEIGHT + 8;
        let n = columns.iter().map(|c| c.4.len()).max().unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..n {
            let mut image = canvas(w, h, BACK);
            text(&mut image, gap as i32 - 2, 2, "PRESENTATIONFLAGS: KIND AND VARIANT, AS THE HANDLER GETS THEM", DIM, None, 2);
            for (k, (name, flags, kind, variant, frames)) in columns.iter().enumerate() {
                let x = gap + k * (WIDTH + gap);
                // The word's kind and variant fields, named as effect.h names
                // them; the handler is handed the same two.
                let (word_kind, word_variant) = (flags & STAGING_KIND_MASK, (flags & STAGING_VARIANT_MASK) >> STAGING_VARIANT_SHIFT);
                assert_eq!((word_kind, word_variant), (*kind, *variant), "{name}: the plan's word and the handler disagree");
                text(&mut image, x as i32 - 2, 22, &name.to_uppercase(), MARK, None, 2);
                text(&mut image, x as i32 - 2, 42, kinds.get(kind).map_or("?", String::as_str), TEXT, None, 2);
                text(&mut image, x as i32 - 2, 60, variants.get(*variant as usize).map_or("?", String::as_str), TEXT, None, 2);
                let f = &frames[i.min(frames.len() - 1)];
                paste(&mut image, &screen(f), x, top, 1);
            }
            out.push(image.pixels);
        }
        let written = super::animation::write(&self.out.join("effect-variants.gif"), &out, w)?;
        self.written.push(("effect-variants.gif".to_string(), written.bytes, format!("Quake at its three variants beside their plan words; {} frames", written.frames)));
        Ok(())
    }

    /// Venus, Mercury, Mars and Jupiter, each summoned with one Djinni on
    /// standby as `render psynergy` summons them, in four panels started
    /// together from the frame each effect starts, as `djinn-attacks.gif`:
    /// the screen beside work->particles' first 32 slots, where the flames
    /// are (lit while a flame moves or counts, the slots its loop walks
    /// outlined), and PARTICLES' first 512, its motes or embers (lit by the
    /// life left in a mote's variant; a Mars ember, which never dies, while
    /// it moves).
    pub fn djinn_attacks(&mut self) -> Result<(), String> {
        // (summon, flames its loop walks)
        let attacks: [(&str, usize); 4] = [("Venus", 10), ("Mercury", 5), ("Mars", 9), ("Jupiter", 8)];
        let ranges = [Range { pool: Pool::Work, first: 0, count: 32 }, Range { pool: Pool::Particles, first: 0, count: 512 }];
        let mut runs = Vec::new();
        for (name, _) in attacks {
            let cast = self.find_cast(name, true)?;
            let (frames, start) = self.record_pools(&cast, &ranges, &[], &mut |_, _, _| {})?;
            eprintln!("djinn-attacks: {name}: {} frames, effect from {start}", frames.len());
            runs.push((frames, start));
        }
        const CELL: usize = 7;
        const COLS: usize = 32;
        let panel_w = WIDTH + 12 + COLS * CELL + 12;
        let panel_h = HEIGHT.max(20 + CELL + 26 + 16 * CELL + 30) + 30;
        let (w, h) = (2 * panel_w + 8, 2 * panel_h + 46);
        // Each attack starts 6 frames before its handler places its
        // particles, after the summon's opening, and ends when its pools fall still for 30 frames; the
        // work block stays taken after that for the summon's Power boost,
        // another effect, which is cut off.
        let mut ends = Vec::new();
        for (frames, _) in &runs {
            let active: Vec<bool> = (0..frames.len()).map(|i| i > 0 && (frames[i].ranges[0] != frames[i - 1].ranges[0] || frames[i].ranges[1].iter().zip(&frames[i - 1].ranges[1]).any(|(a, b)| a != b))).collect();
            let first = active.iter().position(|&a| a).unwrap_or(0);
            let mut end = frames.len();
            let mut still = 0;
            // Mars stands still between placing its embers and its strike.
            for (i, &a) in active.iter().enumerate().skip(first + 60) {
                still = if a { 0 } else { still + 1 };
                if still == 30 {
                    end = i - 20;
                    break;
                }
            }
            eprintln!("  pools active from frame {first}, cut at {end}");
            ends.push((first.saturating_sub(6), end));
        }
        let n = ends.iter().map(|(from, end)| end - from).max().unwrap_or(0);
        let mut out = Vec::new();
        // Every other frame, each shown twice, to keep the file small.
        for i in (0..n).step_by(2) {
            let mut image = canvas(w, h, BACK);
            for (k, (((name, walked), (frames, _)), (from, _))) in attacks.iter().zip(&runs).zip(&ends).enumerate() {
                let (px, py) = (4 + (k % 2) * panel_w, 4 + (k / 2) * panel_h);
                let at = (from + i).min(frames.len() - 1);
                let f = &frames[at];
                let before = &frames[at.saturating_sub(1)];
                text(&mut image, px as i32, py as i32, &name.to_uppercase(), MARK, None, 2);
                paste(&mut image, &screen(&f.screen), px, py + 22, 1);
                let gx = px + WIDTH + 12;
                text(&mut image, gx as i32 - 2, py as i32, "FLAMES: WORK->PARTICLES", HEAD, None, 2);
                let flames: Vec<u32> = (0..32)
                    .map(|s| {
                        let (now, was) = (f.ranges[0][s], before.ranges[0][s]);
                        if !f.running || s >= *walked {
                            FREE
                        } else if now != was {
                            0xff_90_30
                        } else {
                            PARKED
                        }
                    })
                    .collect();
                grid(&mut image, gx, py + 20, COLS, CELL, &flames);
                let my = py + 20 + CELL + 8;
                text(&mut image, gx as i32 - 2, my as i32, "MOTES: PARTICLES", HEAD, None, 2);
                let motes: Vec<u32> = (0..512)
                    .map(|s| {
                        let (now, was) = (f.ranges[1][s], before.ranges[1][s]);
                        if !f.running {
                            FREE
                        } else if *name == "Mars" {
                            if s < 9 * 16 && now != was { 0xff_c0_60 } else if s < 9 * 16 { PARKED } else { FREE }
                        } else if now[3] > 0 {
                            life(now[3], 47, 0xff_e0_70)
                        } else {
                            FREE
                        }
                    })
                    .collect();
                grid(&mut image, gx, my + 18, COLS, CELL, &motes);
                let lit_flames = flames.iter().filter(|&&c| c == 0xff_90_30).count();
                let lit_motes = motes.iter().filter(|&&c| c != FREE && c != PARKED).count();
                text(&mut image, gx as i32 - 2, (my + 18 + 16 * CELL + 6) as i32, &format!("{lit_flames} FLAMES MOVING, {lit_motes} MOTES"), TEXT, None, 2);
            }
            let mut kx = 6;
            let ky = (h - 38) as i32;
            kx += key(&mut image, kx, ky, 0xff_90_30, "FLAME MOVING");
            kx += key(&mut image, kx, ky, PARKED, "PLACED, WAITING");
            kx += key(&mut image, kx, ky, 0xff_e0_70, "MOTE, BY LIFE LEFT");
            key(&mut image, kx, ky, 0xff_c0_60, "MARS EMBER MOVING");
            let ky = ky + 18;
            let kx = 6 + key(&mut image, 6, ky, FREE, "SLOT NOT IN USE");
            text(&mut image, kx, ky - 2, "EACH ROW OF MOTES: 32 SLOTS, ONE FLAME'S", TEXT, None, 2);
            out.push(image.pixels.clone());
            out.push(image.pixels);
        }
        let written = super::animation::write(&self.out.join("djinn-attacks.gif"), &out, w)?;
        self.written.push(("djinn-attacks.gif".to_string(), written.bytes, format!("the four Djinn attacks beside their pools; {} frames", written.frames)));
        Ok(())
    }

    /// Supernova, the spark groups' strongest variant, three bursts 8 frames
    /// apart, beside its pools, as `spark-groups.gif`: work->particles' 48
    /// ring flashes, 16 a burst, lit by the count in their variant (waiting
    /// above 18, drawn from 17 down), and PARTICLES' 576 sparks, 192 a
    /// burst, lit by the life left in theirs.
    pub fn spark_groups(&mut self) -> Result<(), String> {
        let cast = self.find_cast("Supernova", false)?;
        let ranges = [Range { pool: Pool::Work, first: 0, count: 48 }, Range { pool: Pool::Particles, first: 0, count: 576 }];
        let (frames, start) = self.record_pools(&cast, &ranges, &[], &mut |_, _, _| {})?;
        eprintln!("spark-groups: {} frames, effect from {start}", frames.len());
        const CELL: usize = 8;
        const COLS: usize = 16;
        let (w, h) = (2 * WIDTH + 12 + 3 * (COLS * CELL + 12) + 8, 2 * HEIGHT + 34);
        // The handler places every flash, its count 25 or more, before its
        // first frame; before that the slots hold the last effect's.
        let placed = frames.iter().position(|f| f.running && f.ranges[0].iter().take(16).any(|s| s[3] >= 25)).ok_or("the flashes were never placed")?;
        let mut out = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            let before = &frames[i.saturating_sub(1)];
            let mut image = canvas(w, h, BACK);
            paste(&mut image, &screen(&f.screen), 0, 0, 2);
            let mut counts = (0, 0, 0, 0);
            for burst in 0..3usize {
                let x = 2 * WIDTH + 12 + burst * (COLS * CELL + 12);
                text(&mut image, x as i32 - 2, 4, ["FIRST BURST", "SECOND BURST", "THIRD BURST"][burst], MARK, None, 2);
                text(&mut image, x as i32 - 2, 24, "16 FLASHES", HEAD, None, 2);
                let flashes: Vec<u32> = (0..16)
                    .map(|j| {
                        let v = f.ranges[0][burst * 16 + j][3];
                        if !f.running || i < placed {
                            FREE
                        } else if j >= 12 {
                            SPENT
                        } else if v >= 18 {
                            counts.0 += 1;
                            PARKED
                        } else if v >= 0 {
                            counts.1 += 1;
                            life(v, 17, 0xff_ff_ff)
                        } else {
                            FREE
                        }
                    })
                    .collect();
                grid(&mut image, x, 42, COLS, CELL, &flashes);
                text(&mut image, x as i32 - 2, 42 + CELL as i32 + 10, "192 SPARKS", HEAD, None, 2);
                let sparks: Vec<u32> = (0..192)
                    .map(|j| {
                        let v = f.ranges[1][burst * 192 + j][3];
                        let moved = f.ranges[1][burst * 192 + j] != before.ranges[1][burst * 192 + j];
                        if !f.running || i < placed || v <= 0 {
                            FREE
                        } else if !moved {
                            counts.3 += 1;
                            PARKED
                        } else {
                            counts.2 += 1;
                            life(v, 39, 0xff_c0_40)
                        }
                    })
                    .collect();
                grid(&mut image, x, 42 + CELL + 28, COLS, CELL, &sparks);
            }
            let y = (2 * HEIGHT + 10) as i32;
            text(&mut image, 4, y, &format!("FLASHES WAITING {}  DRAWN {}  SPARKS WAITING {}  FLYING {}", counts.0, counts.1, counts.3, counts.2), TEXT, None, 2);
            let kx = 2 * WIDTH as i32 + 12;
            let mut ky = 42 + 3 * CELL as i32 + 28 + 12 * CELL as i32 + 14;
            for (c, label) in [(PARKED, "FLASH WAITING"), (0xff_ff_ff, "FLASH DRAWN, LAST 18 FRAMES"), (SPENT, "LAST 4 OF EACH 16: NEVER DRAWN"), (PARKED, "SPARK PLACED, WAITING"), (0xff_c0_40, "SPARK FLYING, BY LIFE LEFT"), (FREE, "SLOT FREE")] {
                key(&mut image, kx, ky, c, label);
                ky += 16;
            }
            out.push(image.pixels);
        }
        let written = super::animation::write(&self.out.join("spark-groups.gif"), &out, w)?;
        self.written.push(("spark-groups.gif".to_string(), written.bytes, format!("Supernova beside its flashes and sparks; {} frames", written.frames)));
        Ok(())
    }

    /// Debilitate, Impair at its stronger variant, against three foes beside PARTICLES' first
    /// 96 slots, a row of 32 for each target, as `stat-reductions.gif`: a
    /// shard is lit while it moves (it is advanced only after it is drawn),
    /// and the last 8 of each row, which the loop never reaches, are marked.
    pub fn stat_reductions(&mut self) -> Result<(), String> {
        let cast = self.find_cast("Debilitate", false)?;
        let targets = cast.foes.len().max(1);
        let ranges = [Range { pool: Pool::Particles, first: 0, count: 32 * targets as u32 }];
        // The effect's targets in its own order, read as the handler is
        // entered: row j of PARTICLES is effect->actors[j]'s.
        let impair = self.sym("BattleEffect_RunImpair")?;
        let mut actors: Vec<u32> = Vec::new();
        let (frames, start) = self.record_pools(&cast, &ranges, &[impair], &mut |gba, _, _| {
            let effect = gba.register(0);
            let count = gba.read32(effect + EFFECT_COUNT);
            actors = (0..count.min(8)).map(|j| u32::from(gba.read16(effect + EFFECT_ACTORS + 2 * j))).collect();
        })?;
        eprintln!("stat-reductions: {} targets, actors {actors:?}, {} frames, effect from {start}", targets, frames.len());
        const CELL: usize = 12;
        let (w, h) = (2 * WIDTH + 12 + 32 * CELL + 12, 2 * HEIGHT + 30);
        let names: Vec<String> = actors.iter().map(|&u| u.checked_sub(FIRST_ENEMY_UNIT).and_then(|n| cast.foes.get(n as usize)).map_or(format!("UNIT {u}"), |&o| plan::character_name(o).to_uppercase())).collect();
        // The handler places all 512 shards at x 0, over two frames; a shard
        // in flight has left x 0.
        let placed = (1..frames.len()).find(|&i| frames[i].running && frames[i].ranges[0] != frames[i - 1].ranges[0]).unwrap_or(0);
        let mut out = Vec::new();
        let mut first_moved: Vec<Option<usize>> = vec![None; targets];
        let mut flown = vec![false; 32 * targets];
        for (i, f) in frames.iter().enumerate() {
            let before = &frames[i.saturating_sub(1)];
            let mut image = canvas(w, h, BACK);
            paste(&mut image, &screen(&f.screen), 0, 0, 2);
            let x = 2 * WIDTH + 12;
            text(&mut image, x as i32 - 2, 4, "PARTICLES, 32 FOR EACH TARGET", HEAD, None, 2);
            for t in 0..targets {
                let y = 30 + t * (CELL + 36);
                let moving = (0..24).filter(|&s| f.running && i > placed && f.ranges[0][t * 32 + s] != before.ranges[0][t * 32 + s] && f.ranges[0][t * 32 + s][0] != 0).count();
                if moving > 0 && first_moved[t].is_none() {
                    first_moved[t] = Some(i);
                }
                text(&mut image, x as i32 - 2, y as i32, &format!("{} TARGET, {}: 32 SLOTS", ["FIRST", "SECOND", "THIRD", "FOURTH"][t.min(3)], names.get(t).map_or("?", String::as_str)), TEXT, None, 2);
                let cells: Vec<u32> = (0..32)
                    .map(|s| {
                        let (now, was) = (f.ranges[0][t * 32 + s], before.ranges[0][t * 32 + s]);
                        if !f.running {
                            FREE
                        } else if s >= 24 {
                            SPENT
                        } else if i > placed && now != was && now[0] != 0 {
                            flown[t * 32 + s] = true;
                            0xff_70_d0
                        } else if flown[t * 32 + s] {
                            0x6a_40_5c
                        } else {
                            PARKED
                        }
                    })
                    .collect();
                grid(&mut image, x, y + 18, 32, CELL, &cells);
            }
            let ky = (30 + targets * (CELL + 36) + 10) as i32;
            key(&mut image, x as i32, ky, PARKED, "PLACED, NOT YET BROKEN OFF");
            key(&mut image, x as i32, ky + 16, 0xff_70_d0, "SHARD IN FLIGHT");
            key(&mut image, x as i32, ky + 32, 0x6a_40_5c, "SHARD DONE, NO LONGER DRAWN");
            key(&mut image, x as i32, ky + 48, SPENT, "LAST 8 OF EACH 32: NEVER USED");
            key(&mut image, x as i32, ky + 64, FREE, "NOT YET TAKEN BY THE EFFECT");
            let e = i as i32 - start as i32;
            text(&mut image, 4, (2 * HEIGHT + 8) as i32, &format!("{} FRAMES SINCE THE EFFECT BEGAN", e.max(0)), DIM, None, 2);
            out.push(image.pixels);
        }
        eprintln!("  each target's shards first move at frames {:?} (effect from {start})", first_moved);
        let written = super::animation::write(&self.out.join("stat-reductions.gif"), &out, w)?;
        self.written.push(("stat-reductions.gif".to_string(), written.bytes, format!("Debilitate beside each target's shards; {} frames", written.frames)));
        Ok(())
    }

    /// Twin Beaks (two slashes, with sparks) and Acid Bite (three columns,
    /// with smoke), each given to Isaac against a copy of Ivan, beside the
    /// slots BattleEffect_RunTargetStrike parks: work->particles' 64 smoke
    /// puffs, PARTICLES' 128 motes and the 512 sparks from PARTICLES[512],
    /// as `target-strikes.gif`. A slot is lit while its variant is not -1;
    /// the frame a hit lights a group is held, with the slots it lit named.
    pub fn target_strikes(&mut self) -> Result<(), String> {
        let ranges = [Range { pool: Pool::Work, first: 0, count: 64 }, Range { pool: Pool::Particles, first: 0, count: 128 }, Range { pool: Pool::Particles, first: 512, count: 512 }];
        const POOLS: [(&str, usize); 3] = [("64 SMOKE PUFFS: WORK->PARTICLES", 64), ("128 MOTES: PARTICLES", 128), ("512 SPARKS: PARTICLES[512] ON", 512)];
        const CELL: usize = 6;
        const COLS: usize = 32;
        let panel_w = WIDTH * 2 + 12 + COLS * CELL + 76;
        let pools_h = 3 * 20 + (2 + 4 + 16) * CELL + 3 * 6;
        let panel_h = (2 * HEIGHT).max(pools_h) + 44;
        let mut runs = Vec::new();
        for name in ["Twin Beaks", "Acid Bite"] {
            let cast = self.enemy_cast(name)?;
            let (frames, start) = self.record_pools(&cast, &ranges, &[], &mut |_, _, _| {})?;
            eprintln!("target-strikes: {name}: {} frames, effect from {start}", frames.len());
            runs.push((name, frames, start));
        }
        // A hit sets smoke and motes to 0, which only count up from there
        // (smoke is drawn and counted in the hit's own frame), and sparks to
        // 7 to 22, which only count down: a slot leaving -1, or set back to
        // 0 or 1 or upwards, was lit by a hit this frame.
        let relit = |pool: usize, was: i32, v: i32| match pool {
            _ if was == -1 => v != -1,
            0 | 1 => (0..=1).contains(&v) && was > v,
            _ => v > was,
        };
        let lit = |v: i32, pool: usize| match pool {
            0 => (0..=23).contains(&v),
            _ => v >= 0,
        };
        let mut out = Vec::new();
        for (name, frames, start) in &runs {
            // Before the setup parks every slot, they hold what earlier
            // code left there.
            let parked = frames.iter().position(|f| f.running && f.ranges.iter().all(|r| r.iter().all(|s| s[3] == -1))).ok_or("the strike never parked its particles")?;
            let mut hits = 0;
            let mut groups: Vec<usize> = Vec::new();
            for (i, f) in frames.iter().enumerate() {
                let before = &frames[i.saturating_sub(1)];
                let mut image = canvas(panel_w, panel_h, BACK);
                text(&mut image, 4, 4, &name.to_uppercase(), MARK, None, 2);
                paste(&mut image, &screen(&f.screen), 0, 26, 2);
                let x = 2 * WIDTH + 12;
                let mut y = 26;
                let mut newly: Vec<(usize, usize, usize)> = Vec::new();
                for (p, (label, count)) in POOLS.iter().enumerate() {
                    text(&mut image, x as i32 - 2, y as i32, label, HEAD, None, 2);
                    y += 18;
                    let mut fresh: Vec<usize> = Vec::new();
                    let cells: Vec<u32> = (0..*count)
                        .map(|s| {
                            let (v, was) = (f.ranges[p][s][3], before.ranges[p][s][3]);
                            if !f.running || i < parked {
                                FREE
                            } else if i > parked && relit(p, was, v) {
                                fresh.push(s);
                                0xff_ff_ff
                            } else if lit(v, p) {
                                [0xc0_c0_c0, 0x60_e0_90, 0xff_c0_40][p]
                            } else if v == -1 {
                                PARKED
                            } else {
                                SPENT
                            }
                        })
                        .collect();
                    if let Some(&a) = fresh.first() {
                        newly.push((p, fresh.len(), a));
                    }
                    y += grid(&mut image, x, y, COLS, CELL, &cells) + 6;
                }
                let hold = if newly.is_empty() {
                    1
                } else {
                    hits += 1;
                    eprintln!("  frame {} (effect frame {}): lit (pool, count, first slot) {:?}", i, i as i32 - *start as i32, newly);
                    24
                };
                // The group a hit lights, numbered from 1 by its smoke: 16 puffs a
                // group, of which a hit lights the first 8.
                let line = match newly.iter().find(|n| n.0 == 0) {
                    None => String::new(),
                    Some(&(_, _, first)) => {
                        let group = first / 16 + 1;
                        let again = if groups.contains(&group) { " AGAIN" } else { "" };
                        groups.push(group);
                        // A re-lit spark whose new life is no longer than its old is not
                        // seen to change, so a group lit again counts only those seen.
                        let what: Vec<String> = newly.iter().map(|&(p, n, _)| if p == 2 && !again.is_empty() { format!("{n} SPARKS SEEN") } else { format!("{n} {}", ["SMOKE PUFFS", "MOTES", "SPARKS"][p]) }).collect();
                        format!("HIT {hits} LIGHTS GROUP {group}{again}: {}", what.join(", "))
                    }
                };
                text(&mut image, 4, (panel_h - 18) as i32, &line, MARK, None, 2);
                let kx = x as i32;
                let mut ky = y as i32 + 2;
                for (c, label) in [(PARKED, "PARKED, IDLE"), (0xff_ff_ff, "LIT THIS FRAME"), (0xc0_c0_c0, "SMOKE LIT"), (0x60_e0_90, "MOTE LIT"), (0xff_c0_40, "SPARK LIT"), (SPENT, "SMOKE PUFF DONE"), (FREE, "NOT YET PARKED")] {
                    key(&mut image, kx, ky, c, label);
                    ky += 16;
                }
                for _ in 0..hold {
                    out.push(image.pixels.clone());
                }
            }
        }
        let written = super::animation::write(&self.out.join("target-strikes.gif"), &out, panel_w)?;
        self.written.push(("target-strikes.gif".to_string(), written.bytes, format!("Twin Beaks and Acid Bite beside their parked particles; {} frames", written.frames)));
        Ok(())
    }
}
