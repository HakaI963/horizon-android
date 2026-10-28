//! The phone's clipboard, over JNI.
//!
//! `ClipboardManager` is a Java object, and Horizon ships no Java of its own: the
//! activity is Android's `NativeActivity` and the game is arm64 machine code. So the
//! only road to the clipboard is JNI, JNI is `unsafe`, and the crate that owns the
//! clipboard forbids `unsafe_code`. That is why this file hands the engine two plain
//! function pointers and keeps everything else on this side of the seam.
//!
//! A phone has no terminal and no core dump, so nothing here panics: every failure is
//! a line in logcat and an answer that says there is no system clipboard, which the
//! engine takes the way it takes a desktop with no compositor — by pasting its own
//! last copy.

use jni::objects::{JObject, JString, Reference};
use jni::vm::JavaVM;
use jni::strings::JNIStr;
use jni::{Env, JValue, jni_sig, jni_str, sys};

use horizon_client::clipboard::{
    install_exporter as register_exporter, install_opener as register_opener,
    install_picker as register_picker, install_reveal as register_reveal, install_system,
    SystemHooks,
};

use crate::APP;

/// `Context.CLIPBOARD_SERVICE`.
const SERVICE: &str = "clipboard";
/// The label the clip carries, which is what the system's clipboard viewer shows
/// instead of "unknown app".
const LABEL: &str = "Horizon";

/// Register the clipboard with the engine, before the event loop exists.
///
/// Function pointers are all a `forbid(unsafe_code)` crate can be given, so the
/// engine never asks whether the platform has a clipboard: either the hooks are there
/// or the game's own copy is the whole of it.
pub fn install() {
    if !install_system(SystemHooks { get, set }) {
        log::warn!("clipboard: installed twice, keeping the first activity's");
    }
}

/// Hand the engine the activity's browser.
///
/// The same seam as the clipboard, for the same reason: this crate owns the JNI and
/// the engine owns the link. A `false` is the activity's own refusal, which the
/// launcher turns into a log line rather than a dead button.
pub fn install_opener() {
    if !register_opener(open) {
        log::warn!("clipboard: the URL opener was installed twice");
    }
}

/// Hand the engine the activity's file picker, exporter and file manager, beside the
/// browser.
pub fn install_picker() {
    if !register_picker(pick) {
        log::warn!("clipboard: the file picker was installed twice");
    }
    if !register_exporter(export) {
        log::warn!("clipboard: the folder exporter was installed twice");
    }
    if !register_reveal(reveal) {
        log::warn!("clipboard: the file manager was installed twice");
    }
}

/// Call a `(String) -> void` method on the activity. The three file seams are the same
/// call with a different method name, so they share it.
fn call_with_string(method: &'static JNIStr, value: &str) -> bool {
    let Some((vm, activity)) = context() else {
        return false;
    };
    vm.attach_current_thread(|env| -> jni::errors::Result<bool> {
        // SAFETY: the pointer is the activity's global reference, held by `APP`.
        let activity = unsafe { env.as_cast_raw::<JObject<'static>>(&activity) }?;
        let text: JString = env.new_string(value)?;
        env.call_method(
            &*activity,
            method,
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&text)],
        )?
        .v()?;
        Ok(true)
    })
    .unwrap_or(false)
}

/// The picker, with the Download folder as the fallback.
///
/// A picker is a second activity, and a picker started from a native activity on a
/// secondary display does not always come up: the display the game draws on is not the
/// one the picker wants. So the files are copied straight out of the phone's Download
/// folder first — the place a phone user already drops things — and the picker is only
/// offered when there is nothing there to copy. That makes Import work on a device
/// where the dialog never appears.
fn pick(kind: &str) -> bool {
    let copied = import_from_download(kind);
    if copied > 0 {
        log::info!("clipboard: copied {copied} file(s) from Download into import/{kind}");
        return true;
    }
    call_with_string(jni_str!("horizonPickFile"), kind)
}

/// `horizonImportFromDownload`: copies matching files out of Download and answers how
/// many landed, or 0 when there were none.
fn import_from_download(kind: &str) -> i32 {
    let Some((vm, activity)) = context() else {
        return 0;
    };
    vm.attach_current_thread(|env| -> jni::errors::Result<i32> {
        // SAFETY: the pointer is the activity's global reference, held by `APP`.
        let activity = unsafe { env.as_cast_raw::<JObject<'static>>(&activity) }?;
        let text: JString = env.new_string(kind)?;
        let count = env
            .call_method(
                &*activity,
                jni_str!("horizonImportFromDownload"),
                jni_sig!("(Ljava/lang/String;)I"),
                &[JValue::Object(&text)],
            )?
            .i()?;
        Ok(count)
    })
    .unwrap_or(0)
}

fn export(kind: &str) -> bool {
    call_with_string(jni_str!("horizonExportFiles"), kind)
}

fn reveal(relative: &str) -> bool {
    call_with_string(jni_str!("horizonOpenFiles"), relative)
}

fn open(url: &str) -> bool {
    let Some((vm, activity)) = context() else {
        return false;
    };
    vm.attach_current_thread(|env| -> jni::errors::Result<bool> {
        // SAFETY: the pointer is the activity's global reference, held by `APP`.
        let activity = unsafe { env.as_cast_raw::<JObject<'static>>(&activity) }?;
        let url: JString = env.new_string(url)?;
        env.call_method(
            &*activity,
            jni_str!("horizonOpenUrl"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(&url)],
        )?
        .v()?;
        Ok(true)
    })
    .unwrap_or(false)
}

/// The machine, and the activity as the JVM's own global reference.
///
/// `AndroidApp::vm_as_ptr` hands out the same pointer, but unwraps the singleton it
/// reads it from; asking for the singleton keeps a JVM that isn't there a `None`
/// rather than a panic in the middle of the render loop.
fn context() -> Option<(JavaVM, sys::jobject)> {
    let app = APP.get()?;
    let vm = JavaVM::singleton().ok()?;
    Some((vm, app.activity_as_ptr().cast()))
}

/// The activity's `ClipboardManager`, or `None` where the system has none to give.
///
/// Every clipboard on Android is reached through the activity: it is a `Context`, and
/// a `Context` is the only thing that can hand one out.
fn manager<'local, 'other>(
    env: &mut Env<'local>,
    activity: &JObject<'other>,
) -> jni::errors::Result<Option<JObject<'local>>> {
    let service = JObject::from(env.new_string(SERVICE)?);
    let clipboard = env
        .call_method(
            activity,
            jni_str!("getSystemService"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
            &[JValue::Object(&service)],
        )?
        .into_object()?;
    if clipboard.is_null() {
        log::warn!("clipboard: this activity has no {SERVICE} service");
        return Ok(None);
    }
    Ok(Some(clipboard))
}

/// The primary clip as text: the clip, its first item, the item's text.
///
/// A clipboard with nothing on it, a clip with no items, and a clip that is a picture
/// or a file all answer with an empty string, which is the honest "there is no text" —
/// as opposed to `None` from the caller, which means there is no clipboard at all.
fn primary_text<'local, 'other>(
    env: &mut Env<'local>,
    clipboard: &JObject<'other>,
) -> jni::errors::Result<String> {
    let clip = env
        .call_method(
            clipboard,
            jni_str!("getPrimaryClip"),
            jni_sig!("()Landroid/content/ClipData;"),
            &[],
        )?
        .into_object()?;
    if clip.is_null() {
        return Ok(String::new());
    }
    // `getItemAt` throws on an empty clip. A Java exception is caught and turned into
    // an error rather than killing anything, but it is nobody's idea of a clipboard
    // read, and asking how many items there are costs one call.
    let count = env.call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?;
    if count.i()? < 1 {
        return Ok(String::new());
    }
    let item = env
        .call_method(
            &clip,
            jni_str!("getItemAt"),
            jni_sig!("(I)Landroid/content/ClipData$Item;"),
            &[JValue::Int(0)],
        )?
        .into_object()?;
    if item.is_null() {
        return Ok(String::new());
    }
    // `getText` is null for a clip that was never text, and a `CharSequence` is not a
    // `String`, so it is asked for the `String` every Java object can give.
    let text = env
        .call_method(
            &item,
            jni_str!("getText"),
            jni_sig!("()Ljava/lang/CharSequence;"),
            &[],
        )?
        .into_object()?;
    if text.is_null() {
        return Ok(String::new());
    }
    let text = env
        .call_method(
            &text,
            jni_str!("toString"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .into_object()?;
    let string = env.cast_local::<JString>(text)?;
    string.try_to_string(env)
}

/// `KeyboardHandler.getClipboard`, from `ClipboardManager.getPrimaryClip`.
fn get() -> Option<String> {
    let (vm, activity) = context()?;
    match vm.attach_current_thread(|env| -> jni::errors::Result<Option<String>> {
        // SAFETY: the pointer is the activity's own global reference, which lives as
        // long as `APP` holds the `AndroidApp`. `as_cast_raw` borrows it and checks
        // what it points at, so there is no way to delete a reference we don't own.
        let activity = unsafe { env.as_cast_raw::<JObject<'static>>(&activity) }?;
        let Some(clipboard) = manager(env, &*activity)? else {
            return Ok(None);
        };
        Ok(Some(primary_text(env, &clipboard)?))
    }) {
        Ok(text) => text,
        Err(error) => {
            log::warn!("clipboard: cannot read the system clipboard: {error}");
            None
        }
    }
}

/// `KeyboardHandler.setClipboard`, as `ClipData.newPlainText` into `setPrimaryClip`.
fn set(text: &str) -> bool {
    let Some((vm, activity)) = context() else {
        return false;
    };
    match vm.attach_current_thread(|env| -> jni::errors::Result<bool> {
        // SAFETY: as in `get`.
        let activity = unsafe { env.as_cast_raw::<JObject<'static>>(&activity) }?;
        let Some(clipboard) = manager(env, &*activity)? else {
            return Ok(false);
        };
        let label = JObject::from(env.new_string(LABEL)?);
        let copied = JObject::from(env.new_string(text)?);
        let clip = env
            .call_static_method(
                jni_str!("android/content/ClipData"),
                jni_str!("newPlainText"),
                jni_sig!("(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"),
                &[JValue::Object(&label), JValue::Object(&copied)],
            )?
            .into_object()?;
        env.call_method(
            &clipboard,
            jni_str!("setPrimaryClip"),
            jni_sig!("(Landroid/content/ClipData;)V"),
            &[JValue::Object(&clip)],
        )?
        .v()?;
        Ok(true)
    }) {
        Ok(copied) => copied,
        Err(error) => {
            log::warn!("clipboard: cannot copy: {error}");
            false
        }
    }
}
