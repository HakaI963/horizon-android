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

/// The registry's built index. One document, refreshed on every visit to the Mods
/// page rather than cached for days: a mod that has been taken down must stop being
/// offered as soon as the registry says so.
const INDEX: &str =
    "https://raw.githubusercontent.com/ludovic111/horizon-mods/main/.github/index/index.json";

/// The same index at the address the registry's CI publishes it to, tried second: the
/// raw URL is the one that is always there, and a generated one is the one that is
/// cheap.
const INDEX_ALT: &str =
    "https://raw.githubusercontent.com/ludovic111/horizon-mods/main/index.json";

/// Where the registry's files are served from. A download outside this is refused,
/// which is what stops a tampered index pointing the launcher at another host.
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
        .build()
}

/// Fetch and parse the registry. Blocking, so the caller runs it on a worker: the
/// page draws while it happens.
pub fn fetch() -> Registry {
    let agent = agent();
    let mut last = String::from("no attempt made");
    for url in [INDEX, INDEX_ALT] {
        match get(&agent, url) {
            Ok(text) => match parse(&text) {
                Ok(entries) => {
                    return Registry {
                        status: format!(
                            "{} {}",
                            entries.len(),
                            if entries.len() == 1 { "mod" } else { "mods" }
                        ),
                        entries,
                        failed: false,
                    };
                }
                Err(error) => last = error,
            },
            Err(error) => last = error,
        }
    }
    Registry {
        entries: Vec::new(),
        status: format!("Could not reach the mod registry: {last}"),
        failed: true,
    }
}

fn get(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    if !url.starts_with("https://") {
        return Err(format!("refusing a plain-http fetch: {url}"));
    }
    let response = agent.get(url).call().map_err(|error| error.to_string())?;
    let mut text = String::new();
    response
        .into_reader()
        .take(8 << 20)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    Ok(text)
}

/// Read the index into entries. The document is the registry's, so a shape this build
/// does not understand is an error with the field named rather than a silent empty
/// list: an empty list is indistinguishable from "the registry has no mods".
fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let index: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let schema = index.get("schema").and_then(Value::as_u64).unwrap_or(0);
    if schema != 1 {
        return Err(format!(
            "the registry is schema {schema}; this build reads 1"
        ));
    }
    // A file the registry has taken down is refused by hash, whatever the project
    // list says about it.
    let revoked: Vec<(String, Option<String>)> = index
        .get("revoked")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|r| {
                    (
                        r.get("id").and_then(Value::as_str).unwrap_or("").to_owned(),
                        r.get("sha256").and_then(Value::as_str).map(str::to_owned),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let mut entries = Vec::new();
    for project in index
        .get("projects")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        // Only what this build can actually do something with.
        if project.get("kind").and_then(Value::as_str) != Some("mod") {
            continue;
        }
        let id = project.get("id").and_then(Value::as_str).unwrap_or("");
        let Some(version) = project
            .get("versions")
            .and_then(Value::as_array)
            .and_then(|versions| versions.last())
        else {
            continue;
        };
        let game = version.get("game").and_then(Value::as_str).unwrap_or("");
        // The engine plays one version, so offering a mod for another is a broken
        // promise; the page says so instead of installing something that cannot load.
        if game != horizon_core::VANILLA_VERSION {
            continue;
        }
        let Some(file) = version.get("file") else {
            continue;
        };
        let sha256 = file
            .get("sha256")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        if revoked
            .iter()
            .any(|(rid, hash)| rid == id && hash.as_deref().is_none_or(|h| h == sha256))
        {
            continue;
        }
        let Some(url) = file.get("url").and_then(Value::as_str) else {
            continue;
        };
        if !url.starts_with(FILE_HOST) {
            continue;
        }
        entries.push(Entry {
            id: id.to_owned(),
            name: project
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_owned(),
            summary: project
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            version: version
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            game: game.to_owned(),
            url: url.to_owned(),
            sha256,
            size: file.get("size").and_then(Value::as_u64).unwrap_or(0),
            icon: project
                .get("icon")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        });
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
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

