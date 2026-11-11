//! Alchemy's layout: where a game keeps its linker scripts and sources, and
//! where its English listings, each edition's scaffold, its overlays' link
//! scripts and scaffolds and its drafts were kept before recon/ went, as the
//! commands that still read them name them; and the names a listing makes
//! global and the offset a scaffold section places from.

/// An overlay draft named `resource_ID_c_ADDRESS.c` names its function's
/// address less this, the form its listing's labels take.
const OVERLAY_LABEL_OFFSET: u32 = 0x8000;

pub struct Layout {
    pub game: String,
}

impl Layout {
    pub fn new(game: &str) -> Layout {
        Layout { game: game.to_string() }
    }

    /// The game a path under games/ or recon/ belongs to.
    pub fn of_path(path: &str) -> Option<Layout> {
        let path = path.trim_start_matches("./");
        let rest = path.strip_prefix("games/").or_else(|| path.strip_prefix("recon/"))?;
        let game = rest.split('/').next().filter(|game| !game.is_empty())?;
        Some(Layout::new(game))
    }

    /// The linker scripts that place the game's objects, as the Makefile runs
    /// them: the one script, and the overlays' part it includes.
    pub fn linker_scripts(&self) -> [String; 2] {
        [format!("games/{}/ld_script.ld", self.game), format!("games/{}/src/system/overlay/overlay.ld", self.game)]
    }

    /// The folder adopted C goes under.
    pub fn sources(&self) -> String {
        format!("games/{}/src/", self.game)
    }

    /// The folder of the English listings, one `ADDRESS.s` a function.
    pub fn listings(&self) -> String {
        format!("recon/{}/raw", self.game)
    }

    pub fn main_draft(&self, address: &str) -> String {
        format!("recon/{}/en/main/{address}.c", self.game)
    }

    /// The folder of a language's drafts for the main image (`main`) or the
    /// overlays (`overlays`); a language other than English keeps its own
    /// forms there.
    pub fn drafts(&self, language: &str, image: &str) -> String {
        format!("recon/{}/{language}/{image}", self.game)
    }

    /// The two names an overlay function's English draft may have.
    pub fn overlay_drafts(&self, id: &str, address: u32) -> [String; 2] {
        let folder = self.drafts("en", "overlays");
        [
            format!("{folder}/resource_{id}_c_{:08x}.c", address.wrapping_sub(OVERLAY_LABEL_OFFSET)),
            format!("{folder}/resource_{id}/{address:08x}.c"),
        ]
    }

    /// An edition's scaffold object (its source adds `.s`), whose `.rom.OFFSET`
    /// sections each place that edition's bytes from OFFSET of its ROM.
    pub fn scaffold(&self, language: &str) -> String {
        format!("recon/{}/{language}/rom", self.game)
    }

    /// The folder of the overlays' link scripts and scaffolds.
    pub fn overlays(&self) -> String {
        format!("recon/{}/raw/overlays", self.game)
    }

    pub fn overlay_script(&self, id: &str) -> String {
        format!("{}/resource_{id}.ld", self.overlays())
    }
}

/// The names lines make global.
pub fn globals(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix(".global").or_else(|| line.strip_prefix(".globl"))?;
            let name = rest.strip_prefix(|c: char| c.is_whitespace())?.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}


/// The offset a `.rom.OFFSET` section places from.
pub fn rom_offset(section: &str) -> Option<u32> {
    u32::from_str_radix(section.strip_prefix(".rom.")?, 16).ok()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_name_their_game_and_drafts() {
        assert_eq!(Layout::of_path("recon/tla/en/main/080e2264.c").map(|l| l.game).as_deref(), Some("tla"));
        assert_eq!(Layout::of_path("./games/tbs/src/field/field.c").map(|l| l.game).as_deref(), Some("tbs"));
        let layout = Layout::new("tla");
        assert_eq!(layout.overlay_drafts("674", 0x020086f4)[0], "recon/tla/en/overlays/resource_674_c_020006f4.c");
        assert_eq!(layout.linker_scripts()[0], "games/tla/ld_script.ld");
        assert_eq!(globals(&["\t.global F", "\t.globl G", "F:"]), ["F", "G"]);
        assert_eq!(rom_offset(".rom.000e877c"), Some(0xe877c));
    }
}
