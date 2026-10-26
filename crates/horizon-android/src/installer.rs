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
//! This runs before the event loop, because a pack cannot be installed into an
//! engine that has already failed to find one. A progress dialog on the activity
//! is the one surface that exists at that point; the native card needs the loop
//! this code is still deciding to build.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
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
/// the next launch finish the rest. A few thousand small objects over a phone's
/// network is slow; the title must not wait for it.
const SOUND_BUDGET: Duration = Duration::from_secs(20 * 60);

/// Top the pack up with the version's sounds on a background thread, so the game
/// launches silent and fills in. The client JAR has no sounds; they are objects in
/// the asset index, fetched one at a time and checked against their own SHA-1.
/// Files already present are kept, so a pass interrupted by the time budget, a
/// dropped connection or a process death resumes where it stopped.
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

fn fill_sounds(app: &winit::platform::android::activity::AndroidApp) -> Result<usize, String> {
    let target = paths::resources_dir(app);
    if target.join(".sounds-complete").is_file() {
        return Ok(0);
    }
    let manifest = seed_manifest()?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .build();
    let url = manifest
        .get("assetIndex")
        .and_then(|index| index.get("url"))
        .and_then(Value::as_str)
        .ok_or("the version JSON's assetIndex has no url")?;
    let index = get_json(&agent, url)?;
    let objects = index
        .get("objects")
        .and_then(Value::as_object)
        .ok_or("the asset index has no objects")?;
    let deadline = Instant::now() + SOUND_BUDGET;
    let mut got = 0usize;
    let mut missing = 0usize;
    for (name, entry) in objects.iter().filter(|(name, _)| is_sound(name)) {
        if Instant::now() >= deadline {
            return Err("reached the time budget".into());
        }
        let hash = entry.get("hash").and_then(Value::as_str).unwrap_or("");
        let size = entry.get("size").and_then(Value::as_u64).unwrap_or(0);
        let Some(relative) = safe_relative(name) else {
            missing += 1;
            continue;
        };
        let destination = target.join("assets").join(&relative);
        if destination.is_file() && destination.metadata().is_ok_and(|meta| size == 0 || meta.len() == size) {
            continue;
        }
        match fetch_object(&agent, hash, &destination) {
            Ok(()) => got += 1,
            Err(error) => {
                missing += 1;
                log::debug!("minecraft: {name}: {error}");
            }
        }
    }
    if missing == 0 {
        let _ = fs::write(target.join(".sounds-complete"), b"");
        Ok(got)
    } else {
        Err(format!("{missing} sound files still missing"))
    }
}

/// One asset object, checked against the SHA-1 that names it. Retried a few
/// times: this is a few thousand requests over a phone's network, and one dropped
/// connection should not cost the whole sound set.
fn fetch_object(agent: &ureq::Agent, hash: &str, destination: &Path) -> Result<(), String> {
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
    let partial = destination.with_extension("part");
    let mut last = String::from("no attempt made");
    for _ in 0..3 {
        // A total timeout, not just connect/read: one stalled connection must not
        // hold the whole sound set (or the background thread) open indefinitely.
        match agent.get(&url).timeout(Duration::from_secs(60)).call() {
            Ok(response) => match write_verified(response.into_reader(), &partial, hash) {
                Ok(()) => {
                    return fs::rename(&partial, destination)
                        .map_err(|error| format!("{}: {error}", destination.display()));
                }
                Err(error) => last = error,
            },
            Err(error) => last = error.to_string(),
        }
    }
    let _ = fs::remove_file(&partial);
    Err(last)
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
