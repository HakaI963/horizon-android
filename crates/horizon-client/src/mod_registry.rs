//! Horizon's own mods, downloaded from the registry on the phone.
//!
//! The desktop launcher browses the registry over the network with the whole
//! `horizon-mods` index machinery behind it. On a phone the same registry is read
//! here instead, because this crate is the one that already has an HTTP client and a
//! place to put the files, and because the index is a single document that has to be
//! fetched once rather than a live query.
//!
//! What a mod is, and what installing one means, does not change: the registry
//! publishes `.hmod` files with a size and a SHA-256, the file is checked against both
//! before it is renamed into `import/mods/`, and a file the registry has taken down is
//! refused even if it is asked for by name. The launcher never runs a mod — it puts it
//! where the game will find it.
//!
//! The registry's own documents are TOML per project in the repository and one
//! `index.json` built by its CI; only the built index is read here.

use std::fs;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The registry repository, whose `mods/*.toml` files are the entries themselves.
///
/// There is no published `index.json` yet — that file is what the registry's CI
/// builds, and until it exists the entries are the source of truth. Reading them
/// directly also means the launcher sees a mod the moment it is merged rather than
/// when a build catches up.
const REPO: &str = "ludovic111/horizon-mods";
const ENTRIES_API: &str =
    "https://api.github.com/repos/ludovic111/horizon-mods/contents/mods";
const ENTRY_RAW: &str = "https://raw.githubusercontent.com/ludovic111/horizon-mods/main/mods/";

/// Where the registry's files are served from. A download outside this is refused,
/// which is what stops a tampered entry pointing the launcher at another host.
const FILE_HOST: &str = "https://github.com/ludovic111/horizon-mods/releases/download/";

/// One mod as the launcher shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub version: String,
    pub game: String,
    /// The `.hmod` to fetch, and what it must hash to.
    pub url: String,
    pub sha256: String,
    pub size: u64,
    /// The registry's icon address, if it has one. Not fetched here: the page draws a
    /// block for a mod, which needs no network and cannot be a broken image.
    pub icon: String,
}

/// What the Mods page needs: the list, or why there is none.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    pub entries: Vec<Entry>,
    /// A line under the list: "7 mods", or the error that stopped the fetch.
    pub status: String,
    /// Whether the last fetch failed, so the page can offer a retry rather than an
    /// empty list that looks like there is nothing to install.
    pub failed: bool,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(30))
        // GitHub's API answers 403 to a request with no User-Agent, which is what the
        // mod listing is. The Mojang downloads elsewhere do not care, so this agent is
        // the one that says who is asking.
        .user_agent("Horizon-Android")
        .build()
}

/// Fetch the registry. Blocking, so the caller runs it on a worker or accepts the
/// pause: the page is only up when the player asked for it.
pub fn fetch() -> Registry {
    let agent = agent();
    let listing = match get(&agent, ENTRIES_API) {
        Ok(text) => text,
        Err(error) => {
            return Registry {
                entries: Vec::new(),
                status: format!("Could not reach the mod registry: {error}"),
                failed: true,
            }
        }
    };
    let names = match entry_names(&listing) {
        Ok(names) => names,
        Err(error) => {
            return Registry {
                entries: Vec::new(),
                status: format!("The mod registry answered oddly: {error}"),
                failed: true,
            }
        }
    };
    let mut entries = Vec::new();
    let mut last = String::new();
    for name in &names {
        let url = format!("{ENTRY_RAW}{name}");
        let text = match get(&agent, &url) {
            Ok(text) => text,
            Err(error) => {
                last = error;
                continue;
            }
        };
        match parse_entry(&text) {
            Ok(Some(entry)) => entries.push(entry),
            // A mod for another version, or one with no usable file: skipped, not an
            // error. The page says how many there are, and one bad entry must not hide
            // the rest.
            Ok(None) => {}
            Err(error) => last = error,
        }
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    if entries.is_empty() {
        return Registry {
            entries,
            status: if last.is_empty() {
                "The registry has no mods for this build".to_string()
            } else {
                format!("Could not read the mod registry: {last}")
            },
            failed: true,
        };
    }
    Registry {
        status: format!(
            "{} {} from {REPO}",
            entries.len(),
            if entries.len() == 1 { "mod" } else { "mods" }
        ),
        entries,
        failed: false,
    }
}

fn get(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err(format!("refusing a plain-http fetch: {url}"));
    }
    let response = agent.get(url).call().map_err(|error| format!("{url}: {error}"))?;
    let mut text = String::new();
    response
        .into_reader()
        .take(8 << 20)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    Ok(text)
}

/// The `*.toml` file names in the registry's `mods/` folder, from GitHub's contents
/// API. The listing is the only place the set of entries is published, so it is read
/// rather than a list of ids compiled into the app that would go stale.
fn entry_names(listing: &str) -> Result<Vec<String>, String> {
    let items: Value = serde_json::from_str(listing).map_err(|error| error.to_string())?;
    let Some(items) = items.as_array() else {
        return Err("the listing is not an array".into());
    };
    Ok(items
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("file"))
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .filter(|name| name.ends_with(".toml") && !name.starts_with('.'))
        .map(str::to_owned)
        .collect())
}

/// One `mods/<id>.toml` as an [`Entry`], or `None` when this build cannot use it.
///
/// The shape is the registry's own, so it is read with the registry's own type:
/// `horizon_mods::index::registry::Entry` is the definition a submission is reviewed
/// against, which means the launcher cannot drift from it.
fn parse_entry(text: &str) -> Result<Option<Entry>, String> {
    use horizon_mods::index::registry::Entry as RegistryEntry;
    let entry: RegistryEntry = toml::from_str(text).map_err(|error| error.to_string())?;
    if entry.kind != horizon_mods::index::Kind::Mod {
        return Ok(None);
    }
    let Some(version) = entry.versions.first() else {
        return Ok(None);
    };
    // The engine plays one version; offering a mod built for another is a promise it
    // cannot keep.
    if version.game != horizon_core::VANILLA_VERSION {
        return Ok(None);
    }
    let url = format!("{FILE_HOST}{}/{}/{}", entry.id, version.version, version.file);
    Ok(Some(Entry {
        id: entry.id.clone(),
        name: entry.name,
        summary: entry.summary,
        version: version.version.clone(),
        game: version.game.clone(),
        url,
        sha256: version.sha256.clone(),
        // The registry does not record a size in the entry; the file's own length is
        // what the download checks instead.
        size: 0,
        icon: entry.icon.unwrap_or_default(),
    }))
}

/// Where installed mods go: `import/mods/`, the folder the game reads.
pub fn mods_dir(import: &Path) -> PathBuf {
    import.join("mods")
}

/// The mods already on disk, by file name.
pub fn installed(import: &Path) -> Vec<String> {
    let dir = mods_dir(import);
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".hmod"))
        .collect();
    names.sort();
    names
}

/// Download one mod, check it against the registry's size and SHA-256, and move it
/// into place. A file that fails either check is deleted and never renamed: the
/// point of the hash is that a half-written or substituted file does not reach the
/// game.
pub fn install(import: &Path, entry: &Entry) -> Result<PathBuf, String> {
    if entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the registry entry has no usable SHA-256".into());
    }
    if !entry.url.starts_with(FILE_HOST) {
        return Err("the registry entry points outside the registry".into());
    }
    let dir = mods_dir(import);
    fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let name = entry
        .url
        .rsplit('/')
        .next()
        .filter(|name| name.ends_with(".hmod") && !name.contains(".."))
        .ok_or("the registry entry has no file name")?;
    let destination = dir.join(name);
    let partial = dir.join(format!(".{name}.part"));

    let agent = agent();
    let response = agent
        .get(&entry.url)
        .call()
        .map_err(|error| format!("GET {}: {error}", entry.url))?;
    let mut file = BufWriter::new(
        fs::File::create(&partial).map_err(|error| format!("{}: {error}", partial.display()))?,
    );
    let mut digest = Sha256::new();
    let mut written = 0u64;
    let mut buffer = vec![0u8; 1 << 16];
    let mut reader = response.into_reader();
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("reading {}: {error}", entry.url))?;
        if count == 0 {
            break;
        }
        written += count as u64;
        // The index gives a size, so a runaway body is stopped at the boundary rather
        // than filling the phone's storage.
        if entry.size > 0 && written > entry.size {
            let _ = fs::remove_file(&partial);
            return Err("the file is larger than the registry says".into());
        }
        file.write_all(&buffer[..count])
            .map_err(|error| format!("{}: {error}", partial.display()))?;
        digest.update(&buffer[..count]);
    }
    file.flush()
        .map_err(|error| format!("{}: {error}", partial.display()))?;
    drop(file);

    let sha256 = format!("{:x}", digest.finalize());
    if !sha256.eq_ignore_ascii_case(&entry.sha256) {
        let _ = fs::remove_file(&partial);
        return Err("the file does not match the registry's SHA-256".into());
    }
    if entry.size > 0 && written != entry.size {
        let _ = fs::remove_file(&partial);
        return Err("the file is not the size the registry says".into());
    }
    fs::rename(&partial, &destination)
        .map_err(|error| format!("{}: {error}", destination.display()))?;
    Ok(destination)
}

