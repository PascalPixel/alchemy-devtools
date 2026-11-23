//! `render probe scene`: what one scene shows, in either game, read from the
//! running game for naming it (H3): the scene entered by an entrance (its
//! table's or, in The Lost Age, its map's), once the player has control, as
//! a photo, with its entrances and their camera boxes, each actor on stage
//! (where it stands, its sprites) and each row of the trigger table its
//! field holds, named by the function it runs or the message it shows.

use super::field::{result_key, Field, Setup, Symbols, SETTLE};
use super::story::Scene;
use std::collections::HashMap;
use std::path::Path;

/// What the probe is asked: the scene, the entrance (the first way in when
/// none), and the flags to set first; the actors to hide before the photo
/// (--hide-actor), the cells to outline on it (--mark-cells) and to print
/// (--cells), each X0,Y0,X1,Y1 and a layer, and whether the room is drawn
/// whole too (--map).
#[derive(Default)]
pub struct Ask<'a> {
    pub scene: Option<&'a Scene>,
    pub entrance: Option<u16>,
    pub set: Vec<u16>,
    pub hide: Vec<u16>,
    pub mark: Vec<Cells>,
    pub cells: Vec<Cells>,
    pub map: bool,
}

/// A rectangle of map cells, corners included, on one map layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cells {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
    pub layer: u8,
}

impl Cells {
    /// `X0,Y0,X1,Y1` or `X0,Y0,X1,Y1,LAYER` (layer 0 when none), corners in
    /// either order.
    pub fn parse(text: &str) -> Result<Cells, String> {
        let bad = || format!("cells are X0,Y0,X1,Y1 or X0,Y0,X1,Y1,LAYER, not {text:?}");
        let n: Vec<i32> = text.split(',').map(|p| p.trim().parse().map_err(|_| bad())).collect::<Result<_, _>>()?;
        let (layer, corners) = match n[..] {
            [a, b, c, d] => (0, [a, b, c, d]),
            [a, b, c, d, l] if (0..=2).contains(&l) => (l as u8, [a, b, c, d]),
            _ => return Err(bad()),
        };
        Ok(Cells { x0: corners[0].min(corners[2]), z0: corners[1].min(corners[3]), x1: corners[0].max(corners[2]), z1: corners[1].max(corners[3]), layer })
    }
}

/// Outlines each cell of `cells` on `image`, whose top left is map pixel
/// `origin`, at the cell's ground height (`height`, in pixels).
pub fn outline(image: &mut super::field::Image, origin: (i32, i32), cells: &Cells, height: &dyn Fn(i32, i32) -> i32) {
    const MARK: u32 = 0xff00ff;
    for cz in cells.z0..=cells.z1 {
        for cx in cells.x0..=cells.x1 {
            let (left, top) = (cx * 16 - origin.0, cz * 16 - height(cx, cz) - origin.1);
            for i in 0..16 {
                for (x, y) in [(left + i, top), (left + i, top + 15), (left, top + i), (left + 15, top + i)] {
                    if (0..image.width as i32).contains(&x) && (0..image.height as i32).contains(&y) {
                        image.pixels[y as usize * image.width + x as usize] = MARK;
                    }
                }
            }
        }
    }
}

/// Plays the scene and prints what it shows; writes `<scene>-<entrance>.png`
/// into `out` and returns its file name.
pub fn scene(rom: &[u8], symbols: &Symbols, ask: &Ask, functions: &HashMap<String, u32>, messages: &HashMap<u32, String>, out: &Path) -> Result<String, String> {
    let s = ask.scene.ok_or("no scene asked")?;
    let first = Field::enter(rom, symbols, &Setup { scene: s.number, entrance: 0, set: ask.set.clone(), clear: Vec::new() })?;
    let entrances = first.entrances();
    drop(first);
    println!("{} (scene {}, {}):", s.name(), s.number, s.file);
    for e in &entrances {
        let camera = if e.camera == [-1; 4] { "none".to_string() } else { format!("{:?}", e.camera) };
        println!("  entrance {}: ({}, {}), camera box {camera}{}", e.number, e.x, e.z, if e.required_flag >= 0 { format!(", needs flag {:#05x}", e.required_flag) } else { String::new() });
    }
    let entrance = ask.entrance.or_else(|| entrances.iter().find(|e| e.number > 0).or(entrances.first()).map(|e| e.number as u16)).unwrap_or(0);
    let mut field = Field::enter(rom, symbols, &Setup { scene: s.number, entrance, set: ask.set.clone(), clear: Vec::new() })?;
    field.until_control()?;
    field.frames(SETTLE)?;
    if field.scene() != s.number {
        println!("  entered by entrance {entrance}, the game went on to scene {}", field.scene());
    }
    for &f in &ask.set {
        println!("  flag {f:#05x} is {} once the player has control", if field.flag_is_set(f) { "set" } else { "clear" });
    }
    let leader = field.leader().map_or("none".to_string(), |l| format!("({}, {}) layer {}", l.x, l.z, l.layer));
    println!("  entered by entrance {entrance}; the leader at {leader}; the view's top left {:?}", field.view());
    for id in 0..symbols.l.field_actor_count as u16 {
        if field.present(id).is_ok() {
            println!("  {}", super::probe::describe(&field, id));
        }
    }
    let by_address: HashMap<u32, &String> = functions.iter().map(|(n, &a)| (result_key(a), n)).collect();
    for row in field.trigger_rows() {
        let key = result_key(row.result);
        let name = match by_address.get(&key) {
            // An exit row's value is the exit kind its scene's exit table
            // matches.
            _ if row.control & 15 == 1 => format!("exit {}", row.result),
            Some(name) => (*name).clone(),
            None if key < 0x10000 => messages.get(&key).map_or_else(|| format!("message {key:05}"), |m| format!("{m} ({key:05})")),
            None => format!("{:#010x}", row.result),
        };
        let condition = match row.condition {
            -1 => String::new(),
            c if c & 0x1000 != 0 => format!(" while flag {:#05x} is set", c & 0xfff),
            c => format!(" while flag {c:#05x} is clear"),
        };
        println!("  trigger kind {} control {:#06x} target {}{condition}: {name}", row.control & 15, row.control, row.target);
    }
    for &id in &ask.hide {
        field.hide_actor(id);
        println!("  actor {id} hidden");
    }
    if !ask.hide.is_empty() {
        field.frames(2)?;
    }
    // Each cell's behaviour (its collision code), attribute bits and ground
    // height in pixels, by layer and row.
    for c in &ask.cells {
        println!("  cells ({}, {}) to ({}, {}) of layer {}, each code/attributes/height:", c.x0, c.z0, c.x1, c.z1, c.layer);
        println!("    {}", (c.x0..=c.x1).map(|x| format!("{x:>11}")).collect::<String>());
        for cz in c.z0..=c.z1 {
            let mut line = format!("  {cz:>3}");
            for cx in c.x0..=c.x1 {
                let word = field.cell_word(c.layer, cx, cz) as u32;
                let height = field.ground(c.layer, cx * 16 + 8, cz * 16 + 8)? >> 16;
                line.push_str(&format!(" {:>3}/{:02x}/{:<3}", (word >> 16) & 0xff, word >> 24, height));
            }
            println!("{line}");
        }
    }
    let mut heights: HashMap<(u8, i32, i32), i32> = HashMap::new();
    for c in &ask.mark {
        for cz in c.z0..=c.z1 {
            for cx in c.x0..=c.x1 {
                heights.insert((c.layer, cx, cz), field.ground(c.layer, cx * 16 + 8, cz * 16 + 8)? >> 16);
            }
        }
    }
    let mut photo = field.screen();
    let view = field.view();
    for c in &ask.mark {
        outline(&mut photo, view, c, &|x, z| heights.get(&(c.layer, x, z)).copied().unwrap_or(0));
    }
    let file = format!("{}-{entrance}.png", s.slug());
    super::places::write_png(&out.join(&file), &photo)?;
    if ask.map {
        let settled = field.save();
        let bounds = field.bounds();
        let mut map = field.stitch(&settled, bounds, false)?;
        for c in &ask.mark {
            outline(&mut map, (bounds[0] >> 16, bounds[1] >> 16), c, &|x, z| heights.get(&(c.layer, x, z)).copied().unwrap_or(0));
        }
        let name = format!("{}-{entrance}-map.png", s.slug());
        super::places::write_png(&out.join(&name), &map)?;
        println!("  the room whole, leader hidden: {name}");
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_a_rectangle_on_a_layer() {
        assert_eq!(Cells::parse("114,47,110,16").unwrap(), Cells { x0: 110, z0: 16, x1: 114, z1: 47, layer: 0 });
        assert_eq!(Cells::parse("1,2,3,4,2").unwrap().layer, 2);
        assert!(Cells::parse("1,2,3").is_err());
        assert!(Cells::parse("1,2,3,4,5").is_err());
    }

    #[test]
    fn a_marked_cell_is_outlined_at_its_height() {
        let mut image = super::super::field::Image { width: 64, height: 64, pixels: vec![0; 64 * 64] };
        let cell = Cells { x0: 1, z0: 1, x1: 1, z1: 1, layer: 0 };
        // Cell (1, 1) at height 8, the image's top left at map pixel (0, 0):
        // its outline runs from (16, 8) to (31, 23).
        outline(&mut image, (0, 0), &cell, &|_, _| 8);
        assert_eq!(image.pixels[8 * 64 + 16], 0xff00ff);
        assert_eq!(image.pixels[23 * 64 + 31], 0xff00ff);
        assert_eq!(image.pixels[15 * 64 + 20], 0);
    }
}
