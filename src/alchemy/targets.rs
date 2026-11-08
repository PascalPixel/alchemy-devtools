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
    /// The game an edition belongs to: its name before the language.
    pub fn game(&self) -> &'static str {
        self.id.0.split_once('-').map_or(self.id.0, |(game, _)| game)
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

/// Every edition, in rom.sha1's order.
pub fn targets() -> Result<Vec<DecompTarget>, String> {
    Ok(editions()?.iter().map(target).collect())
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
        assert_eq!(targets().unwrap().len(), 12);
        let target = decomp_target(Some("tla-ja")).unwrap();
        assert_eq!((target.game(), target.output_dir, target.rom), ("tla", "out/tla-ja", "roms/tla-ja.gba"));
        assert!(target.verify_reference(b"not the ROM").unwrap_err().contains("rom.sha1"));
        assert!(decomp_target(Some("tbs-xx")).unwrap_err().contains("tbs-en"));
        assert_eq!(parse_decomp_target("tbs-it").unwrap().as_str(), "tbs-it");
    }
}
