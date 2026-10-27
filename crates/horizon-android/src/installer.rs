//! Minecraft Java for the launcher.
//!
//! The game's exporters and data generator call Mojang's own code, so they need a
//! JVM; a phone does not have one, and Horizon does not ship one. CI runs them once
//! for the pinned version and embeds their output in the native library as a seed.
//! The launcher does the rest of what a Java launcher does: download the version's
//! client JAR from Mojang, check the pinned SHA-1, unpack its assets and data, and
//! add the seed. The result is the ordinary prepared pack, assembled on the phone
//! with no Java involved.
//!
//! The seed carries the way to the JAR: the official version JSON names its URL,
//! size and SHA-1, and it is the same JSON the seed's derived files were made from.
//! Nothing is committed: the client JAR stays the player's, and the seed is built
//! from it in CI only.
//!
//! This used to run before the event loop, because a pack cannot be installed into
//! an engine that has already failed to find one. That put a 41 MB download on the
//! thread Android was waiting on for `onStart`, which is an ANR by definition, so the
//! install now runs on the bring-up card's own worker: the loop exists, the card is
//! drawing, and the Java dialog is a progress surface on top of it rather than the
//! only thing that exists. Nothing here needs the loop itself — only `APP`, which is
//! set before either.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use jni::objects::{JObject, JString, Reference};
use jni::strings::JNIStr;
use jni::vm::JavaVM;
use jni::{JValue, jni_sig, jni_str, sys};
use serde_json::Value;
use sha1::{Digest, Sha1};

use crate::APP;
use crate::paths;

/// The JVM-derived half of the prepared pack and the pinned version JSON, made by
/// `.github/workflows/android-apk.yml` and put here by `build.rs`.
const SEED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/seed.bin"));

const CLIENT_CACHE: &str = "client.jar";
/// Mojang's asset objects, named by the first two digits of their SHA-1. The
/// client JAR carries no sounds; every `.ogg` and `sounds.json` is one of these.
const ASSET_OBJECTS: &str = "https://resources.download.minecraft.net";
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// Whether the seed came with this build. A local host build has none.
pub fn seed_available() -> bool {
    !SEED.is_empty()
}

/// Whether the folder holds a pack the engine can open. Checked beyond the manifest
/// so that an interrupted install is noticed and repeated, rather than handed to the
/// engine as a world-less crash.
pub fn installed(resources: &Path) -> bool {
    [
        "horizon-resources.json",
        "facts.json",
        "assets/minecraft/atlases/blocks.json",
        "assets/minecraft/lang/en_us.json",
        "data/minecraft/worldgen/noise_settings/overworld.json",
    ]
    .iter()
    .all(|name| resources.join(name).is_file())
}

/// A retry of the install running on its own thread.
///
/// `install` downloads and unpacks for a minute or more, and the retry used to run it
/// inside `RedrawRequested`: no frame was presented and no event processed until it
/// returned, which on Android is the shape of an ANR. The work is the same; only the
/// thread it runs on changes. The Java dialog is still the progress surface, and it
/// is called from this thread through `attach_current_thread`, which is safe.
pub struct InstallJob {
    done: std::sync::mpsc::Receiver<Result<(), String>>,
}

impl InstallJob {
    /// The result, once. `None` while the install is still running.
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        match self.done.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            // The thread is gone without sending: the process is in a state where
            // nothing can be drawn anyway, so this is not a second error to report.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Some(Err("the install thread stopped".into()))
            }
        }
    }
}

/// Run [`install`] on a named worker thread. `None` when no thread could be made,
/// which the caller reports rather than waiting for a result that never comes.
pub fn spawn_install(app: &winit::platform::android::activity::AndroidApp) -> Option<InstallJob> {
    let (sender, done) = std::sync::mpsc::channel();
    let app = app.clone();
    std::thread::Builder::new()
        .name("horizon-install".into())
        .spawn(move || {
            let _ = sender.send(install(&app));
        })
        .ok()
        .map(|_| InstallJob { done })
}

/// Install the pinned version into the launcher's import folder. The caller shows
/// the result; this owns the progress dialog and the temporary folder.
pub fn install(app: &winit::platform::android::activity::AndroidApp) -> Result<(), String> {
    if !seed_available() {
        return Err("this build has no Minecraft seed; CI builds it and puts it in the APK".into());
    }
    let manifest = seed_manifest()?;
    let version = manifest
        .get("id")
        .and_then(Value::as_str)
        .ok_or("the seed's version JSON has no id")?;
    if version != horizon_core::VANILLA_VERSION {
        return Err(format!(
            "the seed is for Minecraft {version}, Horizon needs {}",
            horizon_core::VANILLA_VERSION
        ));
    }

    start_dialog(&format!(
        "Downloading Minecraft Java {version} from Mojang..."
    ));
    log::info!("minecraft: installing {version} from the official client JAR");

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(120))
        .build();
    let import = paths::import_dir(app);
    fs::create_dir_all(&import).map_err(|error| format!("{}: {error}", import.display()))?;
    clean_stale(&import);
    let downloads = import.join("downloads");
    fs::create_dir_all(&downloads).map_err(|error| format!("{}: {error}", downloads.display()))?;
    let cache = downloads.join(format!("{version}-{CLIENT_CACHE}"));
    let client = download_client(&agent, &manifest, &cache)?;

    let target = paths::resources_dir(app);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let temporary = import.join(format!(".horizon-{}-install", std::process::id()));
    let result = assemble(&client, &temporary);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }

    status_dialog("Finishing the resource pack...");
    let backup = import.join(format!(".horizon-{}-old", std::process::id()));
    if backup.exists() {
        fs::remove_dir_all(&backup).map_err(|error| format!("{}: {error}", backup.display()))?;
    }
    if target.exists() {
        fs::rename(&target, &backup)
            .map_err(|error| format!("cannot move the old pack aside: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, &target) {
        if backup.exists() {
            let _ = fs::rename(&backup, &target);
        }
        let _ = fs::remove_dir_all(&temporary);
        return Err(format!("cannot finish the install: {error}"));
    }
    let _ = fs::remove_dir_all(&backup);
    if !installed(&target) {
        return Err("the installed resource pack is incomplete".into());
    }
    done_dialog();
    log::info!("minecraft: {version} resources are ready at {}", target.display());
    Ok(())
}

/// Remove a crashed install's leftovers. The names are this file's own, and they
/// are the only entries in `import/` that can reach hundreds of megabytes.
fn clean_stale(import: &Path) {
    let Ok(entries) = fs::read_dir(import) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".horizon-")
            && (name.ends_with("-install") || name.ends_with("-old"))
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn assemble(client: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        fs::remove_dir_all(target)
            .map_err(|error| format!("{}: {error}", target.display()))?;
    }
    fs::create_dir_all(target).map_err(|error| format!("{}: {error}", target.display()))?;

    status_dialog("Unpacking Minecraft's textures, models and data...");
    let jar_files = extract_client(client, target)?;
    status_dialog("Adding Horizon's prepared 26.3 data...");
    let seed_files = extract_seed(target)?;
    if !installed(target) {
        return Err("Minecraft's JAR and Horizon's seed did not make a complete pack".into());
    }
    log::info!(
        "minecraft: unpacked {jar_files} client files and {seed_files} prepared files"
    );
    Ok(())
}

fn seed_manifest() -> Result<Value, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(SEED))
        .map_err(|error| format!("the embedded Minecraft seed is not a zip: {error}"))?;
    let mut entry = archive
        .by_name("minecraft-manifest.json")
        .map_err(|error| format!("the embedded seed has no version JSON: {error}"))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|error| format!("the embedded version JSON: {error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("the embedded version JSON: {error}"))
}

fn download_client(
    agent: &ureq::Agent,
    manifest: &Value,
    cache: &Path,
) -> Result<PathBuf, String> {
    let client = manifest
        .get("downloads")
        .and_then(|downloads| downloads.get("client"))
        .ok_or("the version JSON has no client download")?;
    let url = client
        .get("url")
        .and_then(Value::as_str)
        .ok_or("the client download has no URL")?;
    let expected = client
        .get("sha1")
        .and_then(Value::as_str)
        .ok_or("the client download has no SHA-1")?;
    let size = client.get("size").and_then(Value::as_u64);
    if !allowed_url(url) {
        return Err(format!("refusing a client download outside Mojang: {url}"));
    }
    if cache.is_file() {
        let found = file_sha1(cache)?;
        if found == expected {
            log::info!(
                "minecraft: using the checked client JAR at {}",
                cache.display()
            );
            return Ok(cache.to_path_buf());
        }
        log::warn!("minecraft: {} is not the pinned client; downloading it again", cache.display());
    }

    let response = agent
        .get(url)
        .call()
        .map_err(|error| format!("GET {url}: {error}"))?;
    let total = response
        .header("Content-Length")
        .and_then(|length| length.parse::<u64>().ok())
        .or(size);
    let mut reader = response.into_reader();
    let partial = cache.with_extension("jar.part");
    let mut file = BufWriter::new(
        File::create(&partial).map_err(|error| format!("{}: {error}", partial.display()))?,
    );
    let mut digest = Sha1::new();
    let mut written = 0u64;
    let mut buffer = vec![0u8; 1 << 16];
    let mut reported = Instant::now();
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("reading {url}: {error}"))?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count])
            .map_err(|error| format!("{}: {error}", partial.display()))?;
        digest.update(&buffer[..count]);
        written += count as u64;
        if reported.elapsed() >= PROGRESS_EVERY {
            status_dialog(&download_line(written, total));
            reported = Instant::now();
        }
    }
    file.flush()
        .map_err(|error| format!("{}: {error}", partial.display()))?;
    drop(file);
    let actual = format!("{:x}", digest.finalize());
    if actual != expected {
        let _ = fs::remove_file(&partial);
        return Err(format!(
            "the client JAR is not the pinned one: expected {expected}, got {actual}"
        ));
    }
    fs::rename(&partial, cache)
        .map_err(|error| format!("cannot keep the client JAR at {}: {error}", cache.display()))?;
    status_dialog(&download_line(written, total));
    log::info!(
        "minecraft: downloaded the official client JAR ({:.1} MB, sha1 {expected})",
        written as f64 / 1e6
    );
    Ok(cache.to_path_buf())
}

fn download_line(bytes: u64, total: Option<u64>) -> String {
    let version = horizon_core::VANILLA_VERSION;
    match total.filter(|total| *total > 0) {
        Some(total) => format!(
            "Downloading Minecraft Java {version} - {:.1} of {:.1} MB ({:.0}%)",
            bytes as f64 / 1e6,
            total as f64 / 1e6,
            100.0 * bytes as f64 / total as f64
        ),
        None => format!(
            "Downloading Minecraft Java {version} - {:.1} MB",
            bytes as f64 / 1e6
        ),
    }
}

fn allowed_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    host == "mojang.com"
        || host.ends_with(".mojang.com")
        || host == "minecraft.net"
        || host.ends_with(".minecraft.net")
}

/// Whether the game plays this asset: `sounds.json` and everything under `sounds/`.
/// The client JAR has none of them; they are objects in the version's asset index.
fn is_sound(name: &str) -> bool {
    name == "minecraft/sounds.json" || name.starts_with("minecraft/sounds/")
}

/// How long one background pass will spend on sounds before giving up and letting
/// the next launch finish the rest.
///
/// The pool, not this number, is what makes the pass fast now: a few thousand small
/// objects over a phone's network were slow one connection at a time, and a handful
/// of them in parallel finish in seconds. The budget is only the backstop that stops
/// a broken network from holding the background thread forever; a partial pass still
/// resumes from what is already on disk.
const SOUND_BUDGET: Duration = Duration::from_secs(30 * 60);

/// Top the pack up with the version's sounds on a background thread, so the game
/// launches silent and fills in. The client JAR has no sounds; they are objects in
/// the asset index, fetched by a small pool of threads and each checked against its
/// own SHA-1. Files already present are kept, so a pass interrupted by the time
/// budget, a dropped connection or a process death resumes where it stopped.
pub fn top_up_sounds(app: &winit::platform::android::activity::AndroidApp) {
    if !seed_available() {
        return;
    }
    let app = app.clone();
    if std::thread::Builder::new()
        .name("horizon-sounds".into())
        .spawn(move || match fill_sounds(&app) {
            Ok(0) => {}
            Ok(files) => log::info!("minecraft: {files} sound files ready"),
            Err(error) => log::info!("minecraft: sounds still filling in ({error})"),
        })
        .is_err()
    {
        log::warn!("minecraft: no thread to fetch sounds with");
    }
}

/// One sound object from the version's asset index: the name it has there, the
/// SHA-1 that names it, its size, and where it goes under the pack. Owned, so the
/// workers only ever borrow the list and never need a lock.
struct SoundObject {
    name: String,
    hash: String,
    size: u64,
    destination: PathBuf,
}

impl SoundObject {
    /// Whether the file is already on disk at the size the index gives it. A size of
    /// zero in the index means it was not recorded, so the file's existence is all
    /// there is to check.
    fn present(&self) -> bool {
        let Ok(meta) = self.destination.metadata() else {
            return false;
        };
        meta.is_file() && (self.size == 0 || meta.len() == self.size)
    }
}

/// Fetch every sound the version's asset index names that is not already in the
/// pack, on a small pool of threads.
///
/// A few thousand small objects fetched one after another is what made this pass
/// take minutes; a handful of connections at a time turns it into seconds. The list
/// is built once, owned, and only borrowed by the workers; the queue is one atomic
/// index, so there is no lock on the hot path. Files already there are counted before
/// the pool starts, so a resumed pass neither spends a thread on them nor lies about
/// how far along it is.
fn fill_sounds(app: &winit::platform::android::activity::AndroidApp) -> Result<usize, String> {
    let target = paths::resources_dir(app);
    if target.join(".sounds-complete").is_file() {
        return Ok(0);
    }
    let manifest = seed_manifest()?;
    // This agent fetches the index, which is one file. The objects below get an
    // agent each, built by their own worker.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(30))
        .build();
    let url = manifest
        .get("assetIndex")
        .and_then(|index| index.get("url"))
        .and_then(Value::as_str)
        .ok_or("the version JSON's assetIndex has no url")?;
    let index = get_json(&agent, url)?;
    // The title's cube map is seven more objects from the same index, and the JAR's
    // copies are placeholders. Fetching them here means the menu has real art on the
    // next open, and costs one request each.
    match fill_panorama(&agent, &target, &index) {
        Ok(0) => {}
        Ok(faces) => log::info!("minecraft: {faces} title panorama faces ready"),
        Err(error) => log::warn!("minecraft: title panorama not fetched: {error}"),
    }
    let objects = index
        .get("objects")
        .and_then(Value::as_object)
        .ok_or("the asset index has no objects")?;
    let deadline = Instant::now() + SOUND_BUDGET;

    let mut unusable = 0usize;
    let mut sounds = Vec::with_capacity(objects.len());
    for (name, entry) in objects {
        if !is_sound(name) {
            continue;
        }
        let hash = entry.get("hash").and_then(Value::as_str).unwrap_or("");
        let size = entry.get("size").and_then(Value::as_u64).unwrap_or(0);
        let Some(relative) = safe_relative(name) else {
            // Not a path under the pack, so there is nothing to fetch. Counting it
            // missing keeps a pass with one of these from being called complete.
            unusable += 1;
            continue;
        };
        sounds.push(SoundObject {
            name: name.clone(),
            hash: hash.to_string(),
            size,
            destination: target.join("assets").join(relative),
        });
    }

    // A resumed pass is the common one, and most files are already there. Counting
    // them here, on this thread, keeps them out of the pool entirely; the count is
    // reported below so the bar starts at what has actually been done, not at zero.
    let mut work = Vec::with_capacity(sounds.len());
    let mut present = 0usize;
    for sound in &sounds {
        if sound.present() {
            present += 1;
        } else {
            work.push(sound);
        }
    }

    // The total the bar counts against: every sound the index names, including the
    // ones that could not be placed. `begin` resets the count, so it comes before any
    // `step` for the files already done.
    horizon_client::sound_progress::begin((sounds.len() + unusable) as u32);
    let already = unusable + present;
    for _ in 0..already {
        horizon_client::sound_progress::step();
    }

    // Bounded by the phone's cores, by a sane ceiling, and by the files left: a
    // resumed pass with a handful of stragglers should not start sixteen threads
    // for them.
    let threads = std::thread::available_parallelism()
        .map_or(4, |count| count.get())
        .clamp(4, 16)
        .min(work.len());
    let next = AtomicUsize::new(0);
    let got = AtomicUsize::new(0);
    let missing = AtomicUsize::new(unusable);
    let expired = AtomicBool::new(false);
    // What the workers share: the queue, the list, the two counters and the expired
    // flag, all as `&`, plus the deadline as a `Copy` value. `thread::scope` joins
    // every worker before it returns, so these borrows cannot outlive it, and none of
    // the shared pieces needs a lock. The references are bound here rather than inside
    // the scope's body because a worker may only borrow data that outlives the scope.
    let queue = &next;
    let files = work.as_slice();
    let got_count = &got;
    let missing_count = &missing;
    let out_of_time = &expired;
    std::thread::scope(|scope| {
        for _ in 0..threads {
            // `Instant` is `Copy`, so this `move` gives each worker its own copy of
            // the deadline instead of a shared reference.
            scope.spawn(move || {
                // One agent per worker, built here rather than shared: no worker
                // depends on `Agent` being `Sync`, and each keeps its own connection
                // pool for the handful of objects it fetches.
                let agent = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(15))
                    .timeout_read(Duration::from_secs(30))
                    .build();
                loop {
                    if Instant::now() >= deadline {
                        out_of_time.store(true, Ordering::Relaxed);
                        return;
                    }
                    // The queue: whoever takes the next index owns that file. The
                    // list is unique by name, so no two workers share a destination.
                    let index = queue.fetch_add(1, Ordering::Relaxed);
                    let Some(sound) = files.get(index) else {
                        return;
                    };
                    match fetch_object(&agent, &sound.hash, &sound.destination) {
                        Ok(()) => {
                            got_count.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(error) => {
                            missing_count.fetch_add(1, Ordering::Relaxed);
                            log::debug!("minecraft: {}: {error}", sound.name);
                        }
                    }
                    horizon_client::sound_progress::step();
                }
            });
        }
    });

    // The scope has joined, so every worker's stores are visible here.
    let got = got.load(Ordering::Relaxed);
    let missing = missing.load(Ordering::Relaxed);
    // Everything arrived even if the clock ran out on the way: that is a finished
    // pass, not a partial one.
    if missing == 0 && got == work.len() {
        let _ = fs::write(target.join(".sounds-complete"), b"");
        horizon_client::sound_progress::complete();
        return Ok(got);
    }
    horizon_client::sound_progress::stop();
    if expired.load(Ordering::Relaxed) {
        Err("reached the time budget".into())
    } else {
        Err(format!("{missing} sound files still missing"))
    }
}

/// The title's cube map, which the client JAR only carries as a 1×1 grey
/// placeholder: the real faces are objects in the asset index, like the sounds. Without
/// them `panorama::Faces::load` falls back to a flat colour, which is why the title
/// screen was blank.
const PANORAMA: [&str; 7] = [
    "minecraft/textures/gui/title/background/panorama_0.png",
    "minecraft/textures/gui/title/background/panorama_1.png",
    "minecraft/textures/gui/title/background/panorama_2.png",
    "minecraft/textures/gui/title/background/panorama_3.png",
    "minecraft/textures/gui/title/background/panorama_4.png",
    "minecraft/textures/gui/title/background/panorama_5.png",
    "minecraft/textures/gui/title/background/panorama_overlay.png",
];

/// Whether the pack's panorama is real rather than the JAR's placeholder. A
/// placeholder is 1×1, so the face's own size says which one it is; the check reads
/// the PNG header and never decodes the image.
fn panorama_ready(target: &Path) -> bool {
    let dir = target.join("assets/minecraft/textures/gui/title/background");
    PANORAMA.iter().all(|name| {
        let path = dir.join(name.rsplit('/').next().unwrap_or(name));
        std::fs::read(&path)
            .ok()
            .and_then(|data| horizon_client::hud::png_size_bytes(&data))
            .is_some_and(|(w, h)| w > 1 && h > 1)
    })
}

/// Fetch the title's cube map on the same background thread as the sounds, so the
/// menu has real art the second time the app opens. Seven small objects: this is not
/// worth its own thread, and it must not delay the sounds.
fn fill_panorama(agent: &ureq::Agent, target: &Path, index: &Value) -> Result<usize, String> {
    if panorama_ready(target) {
        return Ok(0);
    }
    let objects = index
        .get("objects")
        .and_then(Value::as_object)
        .ok_or("the asset index has no objects")?;
    let mut got = 0usize;
    for name in PANORAMA {
        let Some(entry) = objects.get(name) else {
            return Err(format!("{name} is missing from the asset index"));
        };
        let hash = entry
            .get("hash")
            .and_then(Value::as_str)
            .ok_or("a panorama object has no hash")?;
        let Some(relative) = safe_relative(name) else {
            return Err(format!("{name} is not a safe path"));
        };
        let destination = target.join("assets").join(&relative);
        if destination.is_file() {
            // The placeholder is a real file, so its presence is not the question —
            // its size is. Re-fetching a real face is cheap; leaving a placeholder in
            // place is the bug this exists to fix.
            if let Ok(data) = std::fs::read(&destination)
                && horizon_client::hud::png_size_bytes(&data).is_some_and(|(w, _)| w > 1)
            {
                continue;
            }
        }
        fetch_object(agent, hash, &destination)?;
        got += 1;
    }
    Ok(got)
}

/// One asset object, checked against the SHA-1 that names it. Retried a few
/// times: this is a few thousand requests over a phone's network, and one dropped
/// connection should not cost the whole sound set.
fn fetch_object(agent: &ureq::Agent, hash: &str, destination: &Path) -> Result<(), String> {
    // Attempts, and so how many `.part` names one object can use.
    const ATTEMPTS: usize = 3;
    if hash.len() != 40 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("bad asset hash {hash:?}"));
    }
    let url = format!("{ASSET_OBJECTS}/{}/{hash}", &hash[..2]);
    if !allowed_url(&url) {
        return Err(format!("refusing an asset download outside Mojang: {url}"));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    // A `.part` name per attempt, so a retry never reuses a file an earlier attempt
    // may have left half-written. The destination belongs to this object alone — the
    // work list is unique by name and the pool hands each index to one worker — so no
    // other thread renames onto it. `create_dir_all` above is idempotent, so two
    // workers making the same parent need no lock.
    let attempt_once = |attempt: usize| -> Result<(), String> {
        let partial = partial_path(destination, attempt);
        // A total timeout, not just connect/read: one stalled connection must not
        // hold a worker's slot for the whole budget.
        let response = agent
            .get(&url)
            .timeout(Duration::from_secs(60))
            .call()
            .map_err(|error| error.to_string())?;
        write_verified(response.into_reader(), &partial, hash)?;
        fs::rename(&partial, destination)
            .map_err(|error| format!("{}: {error}", destination.display()))
    };
    let mut last = String::from("no attempt made");
    for attempt in 0..ATTEMPTS {
        match attempt_once(attempt) {
            // The rename moved the partial onto the destination: nothing is left.
            Ok(()) => return Ok(()),
            Err(error) => {
                last = error;
                // `write_verified` removes the file when the SHA-1 is wrong; a read
                // or write error leaves it, and the next attempt uses a new name.
                let _ = fs::remove_file(partial_path(destination, attempt));
            }
        }
    }
    Err(last)
}

/// Where one attempt at `destination` writes before it is renamed into place. The
/// first attempt keeps the plain `.part` name the code has always used, so a file an
/// earlier, interrupted run left behind is overwritten and cleaned up rather than
/// accumulating; later attempts get a number, so a retry never reads a previous
/// attempt's half-written bytes.
fn partial_path(destination: &Path, attempt: usize) -> PathBuf {
    match attempt {
        0 => destination.with_extension("part"),
        attempt => destination.with_extension(format!("{attempt}.part")),
    }
}

/// Stream a body into `partial`, refusing it unless its SHA-1 is `expected`.
fn write_verified<R: Read>(mut reader: R, partial: &Path, expected: &str) -> Result<(), String> {
    let mut file = BufWriter::new(
        File::create(partial).map_err(|error| format!("{}: {error}", partial.display()))?,
    );
    let mut digest = Sha1::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("reading: {error}"))?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count])
            .map_err(|error| format!("{}: {error}", partial.display()))?;
        digest.update(&buffer[..count]);
    }
    file.flush()
        .map_err(|error| format!("{}: {error}", partial.display()))?;
    drop(file);
    let actual = format!("{:x}", digest.finalize());
    if actual != expected {
        let _ = fs::remove_file(partial);
        return Err(format!("sha1 {actual}, expected {expected}"));
    }
    Ok(())
}

fn get_json(agent: &ureq::Agent, url: &str) -> Result<Value, String> {
    if !allowed_url(url) {
        return Err(format!("refusing a download outside Mojang: {url}"));
    }
    let response = agent
        .get(url)
        .timeout(Duration::from_secs(120))
        .call()
        .map_err(|error| format!("GET {url}: {error}"))?;
    let mut text = String::new();
    response
        .into_reader()
        .read_to_string(&mut text)
        .map_err(|error| format!("reading {url}: {error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("{url}: {error}"))
}

/// The archive's name as a path under the pack, or `None` for anything absolute or
/// climbing out of it. Zip entry names are attacker-controlled in the general case:
/// the client JAR comes over the network, so its names get the same check.
fn safe_relative(name: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return None,
            _ => path.push(part),
        }
    }
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path)
    }
}

fn file_sha1(path: &Path) -> Result<String, String> {
    let mut file = BufReader::new(
        File::open(path).map_err(|error| format!("{}: {error}", path.display()))?,
    );
    let mut digest = Sha1::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn extract_client(jar: &Path, target: &Path) -> Result<usize, String> {
    let file = File::open(jar).map_err(|error| format!("{}: {error}", jar.display()))?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|error| format!("{}: {error}", jar.display()))?;
    let mut files = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("{}: {error}", jar.display()))?;
        let wanted = entry.name().starts_with("assets/")
            || entry.name().starts_with("data/")
            || entry.name() == "pack.png";
        if !wanted || entry.is_dir() {
            continue;
        }
        let Some(relative) = safe_relative(entry.name()) else {
            log::warn!("minecraft: skipping an unsafe path in the client JAR");
            continue;
        };
        let destination = target.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let mut out = BufWriter::new(
            File::create(&destination)
                .map_err(|error| format!("{}: {error}", destination.display()))?,
        );
        std::io::copy(&mut entry, &mut out)
            .map_err(|error| format!("{}: {error}", destination.display()))?;
        files += 1;
    }
    Ok(files)
}

fn extract_seed(target: &Path) -> Result<usize, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(SEED))
        .map_err(|error| format!("the embedded Minecraft seed: {error}"))?;
    let mut files = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("the embedded seed: {error}"))?;
        if entry.is_dir() || entry.name() == "minecraft-manifest.json" {
            continue;
        }
        let Some(relative) = safe_relative(entry.name()) else {
            return Err("the embedded seed has an unsafe path".into());
        };
        let destination = target.join(&relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        let mut out = BufWriter::new(
            File::create(&destination)
                .map_err(|error| format!("{}: {error}", destination.display()))?,
        );
        std::io::copy(&mut entry, &mut out)
            .map_err(|error| format!("{}: {error}", destination.display()))?;
        files += 1;
    }
    Ok(files)
}

fn context() -> Option<(JavaVM, sys::jobject)> {
    let app = APP.get()?;
    let vm = JavaVM::singleton().ok()?;
    Some((vm, app.activity_as_ptr().cast()))
}

fn start_dialog(message: &str) {
    dialog(
        jni_str!("horizonInstallStart"),
        "start",
        Some(message),
    );
}

fn status_dialog(message: &str) {
    dialog(
        jni_str!("horizonInstallStatus"),
        "update",
        Some(message),
    );
}

pub fn done_dialog() {
    dialog(jni_str!("horizonInstallDone"), "finish", None);
}

pub fn fail_dialog(message: &str) {
    dialog(
        jni_str!("horizonInstallFailed"),
        "report",
        Some(message),
    );
}

fn dialog(name: &'static JNIStr, what: &str, message: Option<&str>) {
    let Some((vm, activity)) = context() else {
        return;
    };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        // SAFETY: the pointer is the activity's global reference, held by `APP`.
        let activity = unsafe { env.as_cast_raw::<JObject<'static>>(&activity) }?;
        match message {
            Some(message) => {
                let message: JString = env.new_string(message)?;
                env.call_method(
                    &*activity,
                    name,
                    jni_sig!("(Ljava/lang/String;)V"),
                    &[JValue::Object(&message)],
                )?
                .v()?;
            }
            None => {
                env.call_method(&*activity, name, jni_sig!("()V"), &[])?
                    .v()?;
            }
        }
        Ok(())
    });
    if let Err(error) = result {
        log::warn!("minecraft: cannot {what} the install dialog: {error}");
    }
}
