//! The editions, as the checkout's `rom.sha1` lists them: one line each, the
//! SHA-1 of the edition's ROM and the image its build writes
//! (`out/tla-en/tla-en.gba`). The edition is the image's name and builds in
//! the image's folder; its reference ROM is `roms/<edition>.gba`, checked
//! against the same line, as the Makefile checks it.

use crate::compile::root;
use std::sync::OnceLock;

/// An edition's name, such as `tla-en`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecompTargetId(&'static str);

impl DecompTargetId {
    pub fn as_str(self) -> &'static str {
        self.0
    }

    /// The game an edition belongs to: its name before the language.
    pub fn game(self) -> &'static str {
        self.0.split_once('-').map_or(self.0, |(game, _)| game)
    }

    /// The edition's language: its name after the game.
    pub fn language(self) -> &'static str {
        self.0.split_once('-').map_or("", |(_, language)| language)
    }
}

impl std::fmt::Display for DecompTargetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecompTarget {
    pub id: DecompTargetId,
    /// The reference ROM, relative to the checkout.
    pub rom: &'static str,
    /// The folder the edition builds in, relative to the checkout.
    pub output_dir: &'static str,
    rom_sha1: &'static str,
}

impl DecompTarget {
    pub fn game(&self) -> &'static str {
        self.id.game()
    }

    pub fn verify_reference(&self, rom: &[u8]) -> Result<(), String> {
        if format!("{:x}", <sha1::Sha1 as sha1::Digest>::digest(rom)) != self.rom_sha1 {
            return Err(format!("{}: its SHA-1 is not the one rom.sha1 gives {}", self.rom, self.id));
        }
        Ok(())
    }
}

struct Edition {
    id: String,
    rom: String,
    output_dir: String,
    sha1: String,
}

/// rom.sha1's lines: `<sha1>  out/<edition>/<edition>.gba`.
fn read(text: &str) -> Result<Vec<Edition>, String> {
    let mut editions = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut words = line.split_whitespace();
        let (Some(sha1), Some(image)) = (words.next(), words.next()) else {
            return Err(format!("rom.sha1: no hash and image in {line:?}"));
        };
        let image = std::path::Path::new(image);
        let (Some(id), Some(folder)) = (image.file_stem(), image.parent()) else {
            return Err(format!("rom.sha1: no edition in {line:?}"));
        };
        let id = id.to_string_lossy().into_owned();
        editions.push(Edition {
            rom: format!("roms/{id}.gba"),
            output_dir: folder.to_string_lossy().into_owned(),
            sha1: sha1.to_ascii_lowercase(),
            id,
        });
    }
    Ok(editions)
}

fn editions() -> Result<&'static [Edition], String> {
    static EDITIONS: OnceLock<Result<Vec<Edition>, String>> = OnceLock::new();
    EDITIONS
        .get_or_init(|| {
            let path = root().join("rom.sha1");
            read(&std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?)
        })
        .as_deref()
        .map_err(Clone::clone)
}

fn target(edition: &'static Edition) -> DecompTarget {
    DecompTarget {
        id: DecompTargetId(&edition.id),
        rom: &edition.rom,
        output_dir: &edition.output_dir,
        rom_sha1: &edition.sha1,
    }
}

/// Every edition's name, in rom.sha1's order.
pub fn ids() -> Result<Vec<DecompTargetId>, String> {
    Ok(editions()?.iter().map(|edition| DecompTargetId(&edition.id)).collect())
}

/// Every game, in the order rom.sha1 first lists it.
pub fn games() -> Result<Vec<&'static str>, String> {
    let mut games = Vec::new();
    for id in ids()? {
        if !games.contains(&id.game()) {
            games.push(id.game());
        }
    }
    Ok(games)
}

/// A game named on a command line, one rom.sha1 lists.
pub fn game(name: &str) -> Result<&'static str, String> {
    let games = games()?;
    games.iter().copied().find(|&game| game == name).ok_or_else(|| format!("{name:?} is not a game rom.sha1 lists: {}", games.join(" or ")))
}

/// The edition a draft is measured in, and its game: the edition named, or
/// else the English edition of the game named, of the game whose folder the
/// draft's path names (`recon/tla/`), or of rom.sha1's first game.
pub fn draft_edition(game: Option<&'static str>, edition: Option<DecompTarget>, path: &str) -> Result<(&'static str, DecompTarget), String> {
    let edition_game = edition.map(|target| target.game());
    if matches!((game, edition_game), (Some(game), Some(edition_game)) if game != edition_game) {
        return Err("--game and --edition name different games".into());
    }
    let game = match game.or(edition_game) {
        Some(game) => game,
        None => {
            let games = games()?;
            games.iter().copied().find(|game| path.contains(&format!("/{game}/"))).or(games.first().copied()).ok_or("rom.sha1 lists no edition")?
        }
    };
    let target = match edition {
        Some(target) => target,
        None => decomp_target(Some(&format!("{game}-en")))?,
    };
    Ok((game, target))
}

/// The edition a name stands for. A name is made only from rom.sha1's lines,
/// so its edition is always there.
pub fn target_for(id: DecompTargetId) -> DecompTarget {
    let all = editions().expect("an edition's name comes from rom.sha1");
    target(all.iter().find(|edition| edition.id == id.0).expect("an edition's name comes from rom.sha1"))
}

pub fn parse_decomp_target(value: &str) -> Result<DecompTargetId, String> {
    Ok(decomp_target(Some(value))?.id)
}

pub fn decomp_target(id: Option<&str>) -> Result<DecompTarget, String> {
    let id = id.ok_or("name an edition")?;
    let all = editions()?;
    all.iter().find(|edition| edition.id == id).map(target).ok_or_else(|| {
        let expected: Vec<&str> = all.iter().map(|edition| edition.id.as_str()).collect();
        format!("unsupported decomp target {id:?}; expected {}", expected.join(" or "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editions_are_rom_sha1s_lines() {
        let editions = read("5C4695205413DF7DB52B9A184815A07783999971  out/tbs-en/tbs-en.gba\n\nb500663220cb9bf56b9f8e8c0c544f1d6fa3a824  out/tla-en/tla-en.gba\n").unwrap();
        let rows: Vec<_> = editions.iter().map(|e| (e.id.as_str(), e.rom.as_str(), e.output_dir.as_str(), &e.sha1[..4])).collect();
        assert_eq!(rows, [("tbs-en", "roms/tbs-en.gba", "out/tbs-en", "5c46"), ("tla-en", "roms/tla-en.gba", "out/tla-en", "b500")]);
        assert!(read("5c4695205413df7db52b9a184815a07783999971\n").is_err());
    }

    #[test]
    fn the_checkout_lists_twelve_editions_and_checks_a_rom_by_its_line() {
        assert_eq!(ids().unwrap().len(), 12);
        let target = decomp_target(Some("tla-ja")).unwrap();
        assert_eq!((target.game(), target.output_dir, target.rom), ("tla", "out/tla-ja", "roms/tla-ja.gba"));
        assert!(target.verify_reference(b"not the ROM").unwrap_err().contains("rom.sha1"));
        assert!(decomp_target(Some("tbs-xx")).unwrap_err().contains("tbs-en"));
        assert_eq!(parse_decomp_target("tbs-it").unwrap().as_str(), "tbs-it");
        assert_eq!(games().unwrap(), ["tbs", "tla"]);
        assert!(game("gs").unwrap_err().contains("tbs or tla"));
    }

    #[test]
    fn a_draft_is_measured_in_its_games_english_edition_unless_named() {
        let named = |game, edition: Option<&str>, path| {
            draft_edition(game, edition.map(|edition| decomp_target(Some(edition)).unwrap()), path)
                .map(|(game, target)| (game, target.id.as_str()))
        };
        assert_eq!(named(None, None, "recon/tla/en/main/08000000.c").unwrap(), ("tla", "tla-en"));
        assert_eq!(named(None, None, "draft.c").unwrap(), ("tbs", "tbs-en"));
        assert_eq!(named(Some("tla"), None, "draft.c").unwrap(), ("tla", "tla-en"));
        assert_eq!(named(None, Some("tla-de"), "recon/tbs/en/main/08000000.c").unwrap(), ("tla", "tla-de"));
        assert!(named(Some("tbs"), Some("tla-de"), "draft.c").is_err());
    }
}
