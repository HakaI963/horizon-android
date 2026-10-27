//! Settings as a file the player can carry: one ZIP with the loose settings files
//! from the instance's own folder, and the touch layouts beside them.
//!
//! The world backup already knows how to write a ZIP, and a player who moves to a
//! new phone wants the same thing for the things that are not a world: their keys,
//! their video settings, their buttons. Nothing here is Minecraft's own format, so
//! the archive is Horizon's: a flat set of names the importer checks against a list,
//! and a `controls/` folder of layouts.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::Result;

/// The loose files that make up a player's settings, in the order they are read
/// back. Names are the engine's own (`Settings::load`), so the archive is the same
/// thing a desktop install keeps beside `options.txt`.
pub const FILES: [&str; 6] = [
    "options.txt",
    "horizon-graphics.txt",
    "horizon-extras.txt",
    "horizon-controller.txt",
    "horizon.txt",
    "horizon-mods.txt",
];

/// The player name is deliberately absent: it is who a world saved its inventory
/// for, not a preference, and importing it from someone else's archive would
/// silently change the identity a world is opened as.
pub const NOT_EXPORTED: [&str; 1] = ["horizon-player.txt"];

/// The folder name layouts live under inside the archive.
const CONTROLS: &str = "controls";

/// The largest archive this will read. A settings file is a few kilobytes; anything
/// near this is a world backup dropped on the wrong button.
const MAX_ARCHIVE: u64 = 4 << 20;

fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

/// Write the settings beside the instance into `import/`. Returns the archive.
///
/// `home` is the instance's working directory (where `options.txt` lives) and
/// `import` is the folder a file manager can reach, which is also where the touch
/// layout lives.
pub fn export(home: &Path, import: &Path) -> Result<PathBuf> {
    fs::create_dir_all(import)?;
    let base = format!("horizon-settings-{}", stamp());
    let (path, file) = {
        let mut index = 0;
        loop {
            let name = if index == 0 {
                format!("{base}.zip")
            } else {
                format!("{base} ({index}).zip")
            };
            let path = import.join(name);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => break (path, file),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => index += 1,
                Err(e) => return Err(e.into()),
            }
        }
    };
    let result = (|| -> Result<()> {
        let mut zip = zip::ZipWriter::new(file);
        for name in FILES {
            let file = home.join(name);
            if !file.is_file() {
                continue;
            }
            zip.start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            io::copy(&mut fs::File::open(&file)?, &mut zip)?;
        }
        // The layout the player is using, then every preset they saved.
        let layout = import.join("controls.json");
        if layout.is_file() {
            zip.start_file(
                format!("{CONTROLS}/default.json"),
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )?;
            io::copy(&mut fs::File::open(&layout)?, &mut zip)?;
        }
        let presets = import.join(CONTROLS);
        if presets.is_dir() {
            for entry in fs::read_dir(&presets)? {
                let entry = entry?;
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                if !path.is_file() || !name.ends_with(".json") {
                    continue;
                }
                zip.start_file(
                    format!("{CONTROLS}/{name}"),
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )?;
                io::copy(&mut fs::File::open(&path)?, &mut zip)?;
            }
        }
        zip.finish()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result?;
    Ok(path)
}

/// Whether an archive entry is one this importer will write. Anything else — a
/// world, a jar, a path with `..` — is refused rather than skipped, so a mistaken
/// archive says so instead of half-applying.
fn accepted(name: &str) -> bool {
    if FILES.contains(&name) {
        return true;
    }
    let Some(rest) = name.strip_prefix(CONTROLS).and_then(|r| r.strip_prefix('/')) else {
        return false;
    };
    !rest.is_empty()
        && rest.ends_with(".json")
        && !rest.contains('/')
        && !rest.contains('\\')
        && !rest.starts_with('.')
}

/// Read an archive written by [`export`] into the instance. Every entry is checked
/// before a single byte is written: an archive that names one file the importer
/// does not know is refused whole, because a half-imported set of settings is worse
/// than the ones the player already had.
pub fn import(archive: &Path, home: &Path, import: &Path) -> Result<usize> {
    let meta = fs::metadata(archive)?;
    if meta.len() > MAX_ARCHIVE {
        return Err(format!(
            "{} is {} bytes; a settings archive is a few kilobytes",
            archive.display(),
            meta.len()
        )
        .into());
    }
    let file = fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut wanted = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        if !accepted(&name) {
            return Err(format!(
                "{name} is not a settings file; this archive is for something else"
            )
            .into());
        }
        wanted.push((i, name));
    }
    if wanted.is_empty() {
        return Err("the archive holds no settings".into());
    }
    fs::create_dir_all(home)?;
    let mut written = 0usize;
    for (index, name) in wanted {
        let mut entry = zip.by_index(index)?;
        let destination = if let Some(rest) = name.strip_prefix(&format!("{CONTROLS}/")) {
            if rest == "default.json" {
                import.join("controls.json")
            } else {
                let dir = import.join(CONTROLS);
                fs::create_dir_all(&dir)?;
                dir.join(rest)
            }
        } else {
            home.join(&name)
        };
        // Written beside the target and renamed in, so an interrupted import cannot
        // leave a half-written options file behind.
        let partial = destination.with_extension("importing");
        {
            let mut out = fs::File::create(&partial)?;
            io::copy(&mut entry, &mut out)?;
        }
        fs::rename(&partial, &destination)?;
        written += 1;
    }
    Ok(written)
}

/// The newest settings archive in `import/`, which is what the launcher's Import
/// button takes: a phone has no file picker the game can open, so the player drops
/// the archive in the folder and taps once.
pub fn newest(import: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(import).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("horizon-settings-") || !name.ends_with(".zip") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(modified) = meta.modified() else { continue };
        if best.as_ref().is_none_or(|(when, _)| modified > *when) {
            best = Some((modified, entry.path()));
        }
    }
    best.map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("horizon-settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_round_trip_keeps_the_files_it_says_it_does() {
        let home = scratch("home");
        let folder = scratch("import");
        fs::write(home.join("options.txt"), "fov:0.5\n").unwrap();
        fs::write(home.join("horizon-extras.txt"), "minimap:true\n").unwrap();
        // The identity file is not part of settings and must not travel.
        fs::write(home.join("horizon-player.txt"), "name:NotMe\n").unwrap();
        fs::write(folder.join("controls.json"), "{\"scale\":1.0}\n").unwrap();

        let archive = export(&home, &folder).unwrap();
        assert!(archive.is_file());

        fs::write(home.join("options.txt"), "fov:0.0\n").unwrap();
        fs::write(home.join("horizon-player.txt"), "name:Me\n").unwrap();
        let count = import(&archive, &home, &folder).unwrap();
        assert_eq!(count, 3, "two settings files and the layout");
        assert_eq!(fs::read_to_string(home.join("options.txt")).unwrap(), "fov:0.5\n");
        assert_eq!(
            fs::read_to_string(home.join("horizon-player.txt")).unwrap(),
            "name:Me\n",
            "the player name is not settings"
        );
    }

    #[test]
    fn an_archive_naming_an_unknown_file_is_refused_whole() {
        let home = scratch("refuse-home");
        let folder = scratch("refuse-import");
        let path = folder.join("horizon-settings-bad.zip");
        {
            let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            zip.start_file("options.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            io::Write::write_all(&mut zip, b"fov:0.5\n").unwrap();
            zip.start_file("saves/World/level.dat", zip::write::SimpleFileOptions::default())
                .unwrap();
            io::Write::write_all(&mut zip, b"not a level").unwrap();
            zip.finish().unwrap();
        }
        fs::write(home.join("options.txt"), "fov:0.0\n").unwrap();
        assert!(import(&path, &home, &folder).is_err());
        assert_eq!(
            fs::read_to_string(home.join("options.txt")).unwrap(),
            "fov:0.0\n",
            "a refused archive leaves the old settings alone"
        );
    }

    #[test]
    fn the_newest_archive_is_the_one_import_takes() {
        let folder = scratch("newest");
        fs::write(folder.join("horizon-settings-old.zip"), b"x").unwrap();
        fs::write(folder.join("horizon-settings-new.zip"), b"x").unwrap();
        fs::write(folder.join("something-else.zip"), b"x").unwrap();
        let newest = newest(&folder).unwrap();
        assert!(newest.ends_with("horizon-settings-new.zip"), "{newest:?}");
    }
}
