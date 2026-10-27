//! The player's own skin, when it is a file rather than the account's.
//!
//! A phone has no account sign-in, so `skins.rs` has nothing to look up: every
//! player shows the default. This is the other half — a PNG the player dropped in
//! `import/skins/` and picked on the Skins page — kept in `horizon-skin.txt` beside
//! `options.txt` as one `skin:<file>` line, the same shape as `horizon-player.txt`.
//!
//! The choice is a file *name*, never a path: the folder is fixed, so a name that
//! climbed out of it would be reading somebody else's file.

use std::{fs, path::Path};

pub const FILE: &str = "horizon-skin.txt";
const KEY: &str = "skin:";

/// The PNGs a player can pick, sorted, from `dir`. Only files directly inside it:
/// a subfolder is not a skin and walking one is not worth the risk.
pub fn available(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            is_png(&name).then_some(name)
        })
        .collect();
    names.sort();
    names
}

fn is_png(name: &str) -> bool {
    name.len() > 4
        && name.to_ascii_lowercase().ends_with(".png")
        && !name.starts_with('.')
        && !name.contains('/')
        && !name.contains('\\')
}

/// The name kept in `dir`, if it is still one of the files there. A saved choice
/// whose file has been deleted reads as no choice, so the player gets the account
/// skin rather than a missing texture.
pub fn load(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join(FILE)).ok()?;
    let name = text
        .lines()
        .find_map(|line| line.trim_end_matches('\r').strip_prefix(KEY))?
        .trim()
        .to_owned();
    is_png(&name).then_some(name)
}

/// Keep `name` as the choice, or clear it with `None`. Failing to write is not an
/// error worth stopping a game for: the pick still applies for this run.
pub fn save(dir: &Path, name: Option<&str>) {
    let path = dir.join(FILE);
    match name.filter(|name| is_png(name)) {
        Some(name) => {
            if let Err(e) = fs::write(&path, format!("{KEY}{name}\n")) {
                log::warn!("Horizon: could not keep the skin in {}: {e}", path.display());
            }
        }
        None => {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-skin-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_png_files_at_the_top_are_offered() {
        let dir = scratch("list");
        fs::write(dir.join("b.png"), b"x").unwrap();
        fs::write(dir.join("a.PNG"), b"x").unwrap();
        fs::write(dir.join("notes.txt"), b"x").unwrap();
        fs::write(dir.join(".hidden.png"), b"x").unwrap();
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("nested/deep.png"), b"x").unwrap();
        assert_eq!(available(&dir), vec!["a.PNG".to_string(), "b.png".to_string()]);
    }

    #[test]
    fn a_choice_survives_a_round_trip_and_a_deleted_file_is_no_choice() {
        let dir = scratch("choice");
        save(&dir, Some("hero.png"));
        assert_eq!(load(&dir), Some("hero.png".to_string()));
        save(&dir, None);
        assert_eq!(load(&dir), None);
    }

    #[test]
    fn a_name_that_climbs_out_of_the_folder_is_not_a_skin() {
        let dir = scratch("escape");
        fs::write(dir.join(FILE), "skin:../../etc/passwd\n").unwrap();
        assert_eq!(load(&dir), None);
    }
}
