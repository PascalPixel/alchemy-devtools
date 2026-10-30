//! `render probe` questions left open in The Broken Seal's humanization
//! units: Hammet's cell in Lunpa Fortress, Flint's ground on the World Map,
//! the third floor's sockets in Venus Lighthouse, the battle lists' copied
//! tees, an enemy's item from past the enemy units, and how deep Dragon
//! Cloud's and the eruption's motes go. Each probe prints what it read.

use super::guide::Guide;
use super::probe::{cells, describe};
use super::story::Trigger;
use crate::ld_map::Map;

const KEY_A: u32 = 0x1;
const KEY_DOWN: u32 = 0x80;

pub const PROBES: &[&str] = &[
    "hammet",
    "flint",
    "flint-play",
    "venus-sockets-a",
    "list-tees",
    "enemy-slot-509-field",
    "effect-depth",
];

impl<'a> Guide<'a> {
    pub fn probe_unit(&mut self, name: &str, map: &Map) -> Result<(), String> {
        let _ = map;
        match name {
            "hammet" => {
                // LunpaFortressToadonpa (163): Hammet (actor 12) and the
                // cells of step trigger 20 around him.
                for entrance in 0..6u16 {
                    let Ok(mut e) = self.enter_with((163, entrance), &[]) else {
                        println!("entrance {entrance}: cannot enter");
                        continue;
                    };
                    let leader = e.field.leader().ok_or("no leader")?;
                    println!(
                        "entrance {entrance}: leader ({}, {}) layer {}; Hammet {}",
                        leader.x,
                        leader.z,
                        leader.layer,
                        describe(&e.field, 12)
                    );
                    let twenty = e.field.cells_with_code(20)?;
                    println!("  code 20 cells: {twenty:?}");
                    if entrance == 0 {
                        for layer in 0..2 {
                            cells(&e.field, layer, (24, 8), (38, 18));
                        }
                    }
                    let reach = e.field.reach(Some(20))?;
                    println!("  reach stops at codes {:?}", reach.crossing());
                    let spots = match e.field.spots(Trigger::Talk(12)) {
                        Ok(s) => s,
                        Err(err) => {
                            println!("  talk spots: {err}");
                            continue;
                        }
                    };
                    if entrance == 0 {
                        let placed = e.placed_clone();
                        let (image, origin) = super::guide_take::room_map(&mut e.field, &placed)?;
                        super::places::write_png(&self.out.join("hammet-room.png"), &image)?;
                        println!("  room map origin {origin:?}, room {:?}", e.field.room());
                        println!("  ground heights (pixels), columns 24..=38:");
                        for z in 0..=24 {
                            let mut row = Vec::new();
                            for x in 20..=40 {
                                let code = e.field.cell(0, x, z);
                                let h = e.field.ground(0, x * 16 + 8, z * 16 + 8)? >> 16;
                                row.push(if code != 0 {
                                    format!("{:>4}", format!("c{code}"))
                                } else {
                                    format!("{h:4}")
                                });
                            }
                            println!("   {z:3}: {}", row.join(""));
                        }
                        // From the corridor south of the cell's doorway
                        // (28, 18): what the leader can walk to without
                        // crossing step trigger 20.
                        let height = e.field.ground(0, 456, 296)?;
                        let spot = super::field::Spot {
                            x: 456,
                            z: 296,
                            layer: 0,
                            height,
                            facing: 0xc000,
                            walk: 0,
                            cell: None,
                        };
                        e.field.stand(&placed, &spot)?;
                        let inner = e.field.reach(Some(20))?;
                        println!("  from (456, 296){}:", inner.crossing());
                        for z in 4..=20 {
                            let row: String = (20..=40)
                                .map(|x| {
                                    if inner.holds(0, x * 16 + 8, z * 16 + 8) {
                                        '#'
                                    } else {
                                        '.'
                                    }
                                })
                                .collect();
                            println!("   {z:3}: {row}");
                        }
                        for ability in [0x8du16] {
                            match e.field.spots(Trigger::Psynergy(ability, 12)) {
                                Ok(spots) => {
                                    for s in spots.iter().take(6) {
                                        println!("  Psynergy {ability:#x} spot ({}, {}) code {} reached without 20 {}", s.x, s.z, e.field.code_at(0, s.x, s.z), inner.holds(0, s.x, s.z));
                                    }
                                }
                                Err(err) => println!("  Psynergy {ability:#x}: {err}"),
                            }
                        }
                    }
                    for s in spots.iter().take(12) {
                        let code = e.field.code_at(s.layer, s.x, s.z);
                        let held = reach.holds(s.layer, s.x, s.z);
                        let run = e.field.event(
                            super::field::Start::Entered(&e.placed),
                            Trigger::Talk(12),
                            s,
                            &[],
                            &[],
                            4,
                            None,
                        );
                        let pages = run
                            .as_ref()
                            .map(|r| r.pages.iter().map(|p| p.message).collect::<Vec<_>>());
                        println!("  spot ({}, {}) layer {} facing {:#x}: code {code}, walkable without crossing 20 {held}; talk pages {:?}", s.x, s.z, s.layer, s.facing, pages);
                    }
                }
                Ok(())
            }
            "flint" => {
                // The World Map (2) from Vale (entrance 1): what the leader
                // can walk to without crossing Flint's ground (code 90).
                let mut e = self.enter_with((2, 1), &[])?;
                let leader = e.field.leader().ok_or("no leader")?;
                println!(
                    "world map from Vale: leader ({}, {}); Flint {}; Vault area {}",
                    leader.x,
                    leader.z,
                    describe(&e.field, 8),
                    describe(&e.field, 12)
                );
                let reach = e.field.reach(Some(90))?;
                println!("  reach{}", reach.crossing());
                let vault = e.field.actor_address(12);
                let (vx, vz) = (
                    e.field.read32(vault + 8) as i32 >> 16,
                    e.field.read32(vault + 0x10) as i32 >> 16,
                );
                for (dx, dz) in [
                    (0, 0),
                    (-16, 0),
                    (16, 0),
                    (0, -16),
                    (0, 16),
                    (-32, 0),
                    (32, 0),
                    (0, -32),
                    (0, 32),
                ] {
                    println!(
                        "  Vault ({}, {}): reached {}",
                        vx + dx,
                        vz + dz,
                        reach.holds(0, vx + dx, vz + dz)
                    );
                }
                let all = e.field.reach(None)?;
                for (dx, dz) in [(0, 0), (0, 16), (0, 32)] {
                    println!(
                        "  without the stop, Vault ({}, {}): reached {}",
                        vx + dx,
                        vz + dz,
                        all.holds(0, vx + dx, vz + dz)
                    );
                }
                let flint = e.field.cells_with_code(90)?;
                println!(
                    "  code 90 cells ({}): {:?}",
                    flint.len(),
                    flint.iter().take(80).collect::<Vec<_>>()
                );
                Ok(())
            }
            "flint-play" => {
                // From Vale onto Flint's ground, pressing A through whatever
                // comes (his lesson, the tutorial battle, the lesson's second
                // half), with Flint's flag (48) and FLAG_DJINN_LESSON (0x16e)
                // read each second.
                let flags_at = map
                    .symbol("gGameFlagBytes")
                    .map(|(_, a)| a)
                    .ok_or("no gGameFlagBytes")?;
                let flag = |e: &super::guide::Entered, f: u32| {
                    e.field.read8(flags_at + (f >> 3)) >> (f & 7) & 1
                };
                let mut e = self.enter_with((2, 1), &[])?;
                e.field.hold(KEY_DOWN);
                for _ in 0..240 {
                    e.field.frames(1)?;
                }
                let mut last = String::new();
                for second in 0..400 {
                    for k in 0..60 {
                        e.field.hold(if k % 12 < 2 { KEY_A } else { 0 });
                        e.field.frames(1)?;
                    }
                    let l = e.field.leader();
                    let now = format!(
                        "scene {} Flint flag {} lesson {} leader {:?}",
                        e.field.scene(),
                        flag(&e, 48),
                        flag(&e, 0x16e),
                        l.map(|l| (l.x, l.z))
                    );
                    if now != last {
                        println!("  second {second}: {now}");
                        last = now;
                    }
                    if second % 20 == 0 {
                        super::places::write_png(
                            &self.out.join(format!("flint-{second:03}.png")),
                            &e.field.screen(),
                        )?;
                    }
                }
                Ok(())
            }
            "venus-sockets-a" => {
                // VenusLighthouseThirdFloor (185) entered by entrance 15: the
                // pillars as FLAG_VENUS_LIGHTHOUSE_SOCKETS_A records them, the
                // south-east piece (actor 9) in the north-west socket, the
                // west-east piece (10) in the north-east one and the
                // north-west piece (11) a cell east of the south-west one,
                // then pushed west into it. Each light's arrival at a socket
                // (VENUS_ROUTE) and each script a light starts
                // (Actor_StartScript, VENUS_START) are printed.
                let env = |name: &str| {
                    std::env::var(name)
                        .ok()
                        .and_then(|v| u32::from_str_radix(&v, 16).ok())
                        .ok_or(format!("set {name}"))
                };
                let (route, start) = (env("VENUS_ROUTE")?, env("VENUS_START")?);
                let flags_at = map
                    .symbol("gGameFlagBytes")
                    .map(|(_, a)| a)
                    .ok_or("no gGameFlagBytes")?;
                let mut e = self.enter_with((185, 15), &[])?;
                for (id, x, z) in [(9u16, 776, 664), (10, 840, 664), (11, 792, 696)] {
                    let a = e.field.actor_address(id);
                    e.field.write32(a + 8, (x << 16) as u32);
                    e.field.write32(a + 0x10, (z << 16) as u32);
                }
                e.field.frames(2)?;
                let pillars: Vec<u32> = (9..12).map(|i| e.field.actor_address(i)).collect();
                e.field.probe_pcs = vec![route, start];
                e.field.probe = Some(Box::new(
                    move |gba: &super::mgba::Gba, pc: u32, frame: u32| {
                        if pc == route {
                            let light = gba.register(0);
                            let (lx, lz) = (
                                gba.read32(light + 8) as i32 >> 16,
                                gba.read32(light + 0x10) as i32 >> 16,
                            );
                            let stands: Vec<String> = pillars
                                .iter()
                                .map(|&p| {
                                    format!(
                                        "({}, {}, y {})",
                                        gba.read32(p + 8) as i32 >> 16,
                                        gba.read32(p + 0x10) as i32 >> 16,
                                        gba.read32(p + 0x0c) as i32 >> 16
                                    )
                                })
                                .collect();
                            println!("  frame {frame}: light {light:08x} reaches ({lx}, {lz}), cell ({}, {}); pillars {}", lx >> 4, lz >> 4, stands.join(" "));
                        } else if pc == start {
                            println!(
                                "  frame {frame}: Actor_StartScript({:08x}, {:08x}) from {:08x}",
                                gba.register(0),
                                gba.register(1),
                                gba.register(14)
                            );
                        }
                    },
                ));
                let height = e.field.ground(0, 808, 696)?;
                e.field.put_leader(&super::field::Spot {
                    x: 808,
                    z: 696,
                    layer: 0,
                    height,
                    facing: 0x8000,
                    walk: 0,
                    cell: None,
                });
                e.field.frames(4)?;
                e.field.hold(0x20);
                e.field.frames(40)?;
                e.field.hold(0);
                for k in 0..1200 {
                    e.field.frames(1)?;
                    if k % 100 == 0 {
                        super::places::write_png(
                            &self.out.join(format!("venus-a-{k:04}.png")),
                            &e.field.screen(),
                        )?;
                    }
                }
                e.field.probe = None;
                let flag = |f: u32| e.field.read8(flags_at + (f >> 3)) >> (f & 7) & 1;
                for name in [
                    "FLAG_VENUS_LIGHTHOUSE_THREE_PILLAR_GATE_OPEN",
                    "FLAG_VENUS_LIGHTHOUSE_SOCKETS_A",
                    "FLAG_VENUS_LIGHTHOUSE_SOCKETS_B",
                ] {
                    println!("  {name} {}", flag(u32::from(self.flag(name)?)));
                }
                for id in 9..12 {
                    println!("  {}", describe(&e.field, id));
                }
                // The player has control again: walk a little.
                e.field.hold(0x40);
                e.field.frames(30)?;
                e.field.hold(0);
                println!(
                    "  leader after walking north: {:?}; scene {}",
                    e.field.leader().map(|l| (l.x, l.z)),
                    e.field.scene()
                );
                Ok(())
            }
            "list-tees" => {
                // Isaac's battle Psynergy list and item list, opened from the
                // command menu as `render psynergy` sets the battle up, then
                // closed with B: each BG tilemap entry of a shown layer whose
                // tile covers the bytes of BG character block 0's 4bpp tiles
                // 512, 513, 528 and 529, where the lists copy the border
                // tees; those bytes as the command row has them, with the
                // list open and after it closes; and the pixels that change
                // when they are painted over while the list is open.
                const KEY_B: u32 = 0x2;
                const KEY_LEFT: u32 = 0x20;
                const KEY_RIGHT: u32 = 0x10;
                const TILES: [u32; 4] = [512, 513, 528, 529];
                let root = crate::compile::root();
                let plan = super::plan::Tables::read(root)?.plan()?;
                let cast = plan
                    .casts
                    .into_iter()
                    .find(|c| c.ability.name == "Quake")
                    .ok_or("no Quake cast")?;
                let symbols = super::battle::Symbols::find(self.map)?;
                // Each script opens its list, waits OPEN frames, and closes it.
                const OPEN: u32 = 120;
                let psynergy: [(u32, u32); 9] = [
                    (0, 60),
                    (KEY_A, 4),
                    (0, 40),
                    (KEY_RIGHT, 4),
                    (0, 30),
                    (KEY_A, 4),
                    (0, OPEN),
                    (KEY_B, 4),
                    (0, 120),
                ];
                let items: [(u32, u32); 11] = [
                    (0, 60),
                    (KEY_A, 4),
                    (0, 40),
                    (KEY_LEFT, 4),
                    (0, 30),
                    (KEY_LEFT, 4),
                    (0, 30),
                    (KEY_A, 4),
                    (0, OPEN),
                    (KEY_B, 4),
                    (0, 120),
                ];
                for (name, menu) in [("psynergy", &psynergy[..]), ("items", &items[..])] {
                    let total = menu.iter().map(|m| m.1 as usize).sum::<usize>();
                    // A frame late in the list's OPEN frames.
                    let open = total - 124 - 20;
                    let mut plain: Vec<Vec<u32>> = Vec::new();
                    for paint in [false, true] {
                        let mut seen: std::collections::BTreeMap<String, usize> =
                            std::collections::BTreeMap::new();
                        let mut frame = 0usize;
                        let mut pictures: Vec<Vec<u32>> = Vec::new();
                        super::battle::record_watching(
                            self.rom,
                            &symbols,
                            &cast,
                            &super::battle::Scene::Game,
                            false,
                            Some(menu),
                            &[],
                            &mut |gba, hook| {
                                if hook.is_some() {
                                    return;
                                }
                                frame += 1;
                                let dispcnt = gba.read16(0x0400_0000);
                                for bg in 0..2u32 {
                                    if dispcnt >> (8 + bg) & 1 == 0 {
                                        continue;
                                    }
                                    let cnt = gba.read16(0x0400_0008 + 2 * bg);
                                    let char_base = u32::from(cnt >> 2 & 3);
                                    let screen = 0x0600_0000 + u32::from(cnt >> 8 & 31) * 0x800;
                                    let size = if cnt >> 7 & 1 == 1 { 64 } else { 32 };
                                    for i in 0..[1, 2, 2, 4][usize::from(cnt >> 14)] * 1024 {
                                        let entry = gba.read16(screen + 2 * i);
                                        let address =
                                            char_base * 0x4000 + u32::from(entry & 0x3ff) * size;
                                        if !paint
                                            && TILES.iter().any(|&t| {
                                                (address..address + size).contains(&(t * 32))
                                            })
                                        {
                                            let line = format!("BG{bg} ({}bpp) map entry ({}, {}) names tile {}; DISPCNT {dispcnt:04x}", if size == 64 { 8 } else { 4 }, i % 32, i / 32, entry & 0x3ff);
                                            *seen.entry(line).or_insert(0) += 1;
                                        }
                                    }
                                }
                                if !paint && (frame == 100 || frame == open || frame + 4 == total) {
                                    let words: Vec<String> = TILES
                                        .iter()
                                        .map(|&t| {
                                            (0..8)
                                                .map(|w| {
                                                    format!(
                                                        "{:08x}",
                                                        gba.read32(0x0600_0000 + t * 32 + 4 * w)
                                                    )
                                                })
                                                .collect::<Vec<_>>()
                                                .join("")
                                        })
                                        .collect();
                                    println!("  frame {frame} (DISPCNT {dispcnt:04x}): tiles 512, 513, 528, 529 hold {}", words.join(" | "));
                                }
                                if frame > open && frame <= open + 3 {
                                    pictures.push(gba.picture_rgb());
                                }
                                if paint && frame == open {
                                    for t in TILES {
                                        for w in 0..8 {
                                            gba.write32(0x0600_0000 + t * 32 + 4 * w, 0xffff_ffff);
                                        }
                                    }
                                }
                            },
                        )?;
                        if !paint {
                            println!("{name}:");
                            for (line, n) in &seen {
                                println!("  {line}: {n} frames");
                            }
                            plain = pictures;
                            continue;
                        }
                        for (k, (a, b)) in plain.iter().zip(&pictures).enumerate() {
                            let changed = a.iter().zip(b).filter(|(x, y)| x != y).count();
                            println!(
                                "  painted while open, frame {}: {changed} pixels change",
                                open + 1 + k
                            );
                        }
                        if let Some(b) = pictures.first() {
                            super::places::write_png(
                                &self.out.join(format!("list-tees-{name}.png")),
                                &super::field::Image {
                                    width: super::mgba::WIDTH,
                                    height: super::mgba::HEIGHT,
                                    pixels: b.clone(),
                                },
                            )?;
                        }
                    }
                }
                Ok(())
            }
            "enemy-slot-509-field" => {
                // Random battles walked into in Goma Cave (27) and
                // fought with A: at each Battle_BuildOpponentActions entry,
                // the halfword each enemy unit's slot 509 reads, and the
                // heap slot it lies in.
                let at = |s: &str| {
                    map.symbol(s)
                        .map(|(_, a)| a & !1)
                        .ok_or_else(|| format!("the map has no {s}"))
                };
                let (units, slots, opponents) = (
                    at("gEnemyUnits")?,
                    at("gHeapSlots")?,
                    at("Battle_BuildOpponentActions")?,
                );
                for place in [(27u16, 5u16)] {
                    let Ok(mut e) = self.enter_with(place, &[]) else {
                        println!("{place:?}: cannot enter");
                        continue;
                    };
                    let words: Vec<String> = [0x0203_112eu32, 0x0203_127a, 0x0203_13c6]
                        .iter()
                        .map(|&a| format!("{a:08x}={:04x}", e.field.read16(a)))
                        .collect();
                    println!("{place:?} in the field: {}", words.join(" "));
                    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
                    let keep = seen.clone();
                    e.field.probe_pcs = vec![opponents];
                    e.field.probe = Some(Box::new(
                        move |gba: &super::mgba::Gba, pc: u32, frame: u32| {
                            if pc != opponents {
                                return;
                            }
                            let block = gba.read32(units);
                            let table: Vec<u32> =
                                (0..64).map(|s| gba.read32(slots + 4 * s)).collect();
                            let values: Vec<String> = (0..6u32)
                                .map(|n| {
                                    let address = block + n * 0x14c + 0xd8 + 1018;
                                    let holder = (0..64)
                                        .filter(|&s| table[s] != 0 && table[s] <= address)
                                        .max_by_key(|&s| table[s])
                                        .unwrap_or(99);
                                    format!("{:04x} (slot {holder})", gba.read16(address))
                                })
                                .collect();
                            keep.borrow_mut().push(format!(
                                "frame {frame}: block {block:08x}; slot 509 of enemies 0 to 5: {}",
                                values.join(", ")
                            ));
                        },
                    ));
                    let mut fought = 0;
                    'walk: for leg in 0..600 {
                        for keys in [
                            if leg % 2 == 0 { 0x40 } else { 0x80 },
                            if leg % 2 == 0 { 0x20 } else { 0x10 },
                        ] {
                            e.field.hold(keys | 0x2);
                            for _ in 0..70 {
                                e.field.frames(1)?;
                                if e.field.scene() == 0x1fe {
                                    fought += 1;
                                    for _ in 0..400 {
                                        e.field.hold(KEY_A);
                                        e.field.frames(3)?;
                                        e.field.hold(0);
                                        e.field.frames(9)?;
                                        if e.field.scene() != 0x1fe {
                                            break;
                                        }
                                    }
                                    if fought >= 3 {
                                        break 'walk;
                                    }
                                }
                            }
                        }
                    }
                    println!("{place:?}: {fought} battles");
                    for line in seen.borrow().iter().take(12) {
                        println!("  {line}");
                    }
                }
                Ok(())
            }
            "effect-depth" => {
                // Dragon Cloud and the eruption (Blast, Mad Blast, Fiery
                // Blast, Smoke Bomb, Sleep Bomb), cast as `render psynergy`
                // casts them: for each call of Projection_Point from those
                // effects, the depth the effect then reads from the point
                // (struct ScreenPoint's third word, after the effect's clamp
                // to 160..799), by call site.
                let root = crate::compile::root();
                let plan = super::plan::Tables::read(root)?.plan()?;
                let symbols = super::battle::Symbols::find(self.map)?;
                let at = |s: &str| {
                    map.symbol(s)
                        .map(|(_, a)| a & !1)
                        .ok_or_else(|| format!("the map has no {s}"))
                };
                let project = at("Projection_Point")?;
                let ranges = [
                    (
                        at("BattleEffect_RunDragonCloud")?,
                        at("BattleEffect_RunAnnihilation")?,
                    ),
                    (
                        at("BattleEffect_RunEruption")?,
                        at("BattleEffect_SpawnObjects")?,
                    ),
                ];
                for name in [
                    "Dragon Cloud",
                    "Blast",
                    "Mad Blast",
                    "Fiery Blast",
                    "Smoke Bomb",
                    "Sleep Bomb",
                ] {
                    let cast = plan
                        .casts
                        .iter()
                        .find(|c| c.ability.name == name)
                        .cloned()
                        .or_else(|| {
                            plan.skipped
                                .iter()
                                .find(|(a, _)| a.name == name)
                                .and_then(|(a, _)| super::plan::cast(a.clone(), false))
                        });
                    let Some(cast) = cast else {
                        println!("{name}: no cast");
                        continue;
                    };
                    // Per call site: calls, the deepest depth read, how many at 736 or deeper.
                    let mut sites: std::collections::BTreeMap<u32, (usize, i32, i32, usize)> =
                        std::collections::BTreeMap::new();
                    let mut last: Option<(u32, u32, u32)> = None;
                    let mut particles: std::collections::BTreeMap<(u32, u32), (usize, usize)> =
                        std::collections::BTreeMap::new();
                    let settle = |gba: &super::mgba::Gba,
                                  last: &mut Option<(u32, u32, u32)>,
                                  sites: &mut std::collections::BTreeMap<
                        u32,
                        (usize, i32, i32, usize),
                    >,
                                  particles: &mut std::collections::BTreeMap<
                        (u32, u32),
                        (usize, usize),
                    >| {
                        if let Some((pointer, site, particle)) = last.take() {
                            let depth = gba.read32(pointer + 8) as i32;
                            let p = particles.entry((site, particle)).or_insert((0, 0));
                            p.0 += 1;
                            if depth >= 736 {
                                p.1 += 1;
                            }
                            let e = sites.entry(site).or_insert((0, i32::MIN, i32::MAX, 0));
                            e.0 += 1;
                            e.1 = e.1.max(depth);
                            e.2 = e.2.min(depth);
                            if depth >= 736 {
                                e.3 += 1;
                            }
                        }
                    };
                    let recording = super::battle::record_watching(
                        self.rom,
                        &symbols,
                        &cast,
                        &super::battle::Scene::Game,
                        false,
                        None,
                        &[project],
                        &mut |gba, hook| {
                            settle(gba, &mut last, &mut sites, &mut particles);
                            if hook == Some(project) {
                                let lr = gba.register(14) & !1;
                                if ranges.iter().any(|&(a, b)| (a..b).contains(&lr)) {
                                    last = Some((gba.register(1), lr, gba.register(0)));
                                }
                            }
                        },
                    )?;
                    println!(
                        "{name} ({} frames), cast by {}:",
                        recording.frames.len(),
                        super::plan::character_name(cast.caster())
                    );
                    for (site, (n, deepest, nearest, deep)) in &sites {
                        println!("  from {site:08x}: {n} points, depth {nearest}..{deepest}, {deep} at 736 or deeper");
                        let mine: Vec<(u32, (usize, usize))> = particles
                            .iter()
                            .filter(|((s, _), _)| s == site)
                            .map(|((_, p), v)| (*p, *v))
                            .collect();
                        let deep_at: Vec<String> = mine
                            .iter()
                            .enumerate()
                            .filter(|(_, (_, v))| v.1 > 0)
                            .map(|(i, (p, v))| format!("#{i} {p:08x}: {}/{}", v.1, v.0))
                            .collect();
                        println!(
                            "    {} particles; deep ones (by address order): {}",
                            mine.len(),
                            deep_at.join(", ")
                        );
                    }
                }
                Ok(())
            }
            other => Err(format!("no probe {other}")),
        }
    }
}
