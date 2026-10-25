//! The phone's soft keyboard, over JNI.
//!
//! winit can raise and lower Android's input method, but the words a phone's
//! keyboard commits go to a focused editable View, and Android's `NativeActivity`
//! has none: its window surface is taken away from the view system and handed to
//! the engine. So the activity is a Java subclass, `HorizonActivity`, that keeps a
//! 1x1 transparent `EditText` for exactly that purpose, and this file asks it for
//! the field's whole text and its keys a frame at a time.
//!
//! The engine forbids `unsafe_code` and JNI is `unsafe`, so it gets function
//! pointers and never sees a `JNIEnv`. Nothing here draws and nothing here is a
//! game field: the view is a mailbox. Every failure is a line in logcat and an
//! empty answer, which the engine takes as "nothing was typed".

use jni::objects::{JObject, JString, Reference};
use jni::vm::JavaVM;
use jni::{jni_sig, jni_str, sys};

use horizon_client::soft_keyboard::{install_system, SystemHooks};

use crate::APP;

/// Register the soft keyboard with the engine, before the event loop exists.
pub fn install() {
    if !install_system(SystemHooks {
        show,
        hide,
        reset,
        text,
        keys,
    }) {
        log::warn!("keyboard: installed twice, keeping the first activity's");
    }
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

fn show() {
    call("show", jni_str!("horizonShowKeyboard"));
}

fn hide() {
    call("hide", jni_str!("horizonHideKeyboard"));
}

fn reset() {
    call("reset", jni_str!("horizonResetKeyboard"));
}

/// One void method on the activity: all three keyboard commands are shaped alike.
fn call(what: &str, name: &'static jni::strings::JNIStr) {
    let Some((vm, activity)) = context() else {
        return;
    };
    if let Err(error) = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        // SAFETY: the pointer is the activity's own global reference, which lives as
        // long as `APP` holds the `AndroidApp`. `as_cast_raw` borrows it and checks
        // what it points at, so there is no way to delete a reference we don't own.
        let activity = unsafe { env.as_cast_raw::<JObject<'static>>(&activity) }?;
        env.call_method(&*activity, name, jni_sig!("()V"), &[])?
            .v()?;
        Ok(())
    }) {
        log::warn!("keyboard: cannot {what} the keyboard: {error}");
    }
}

/// Everything the platform's field holds now, or an empty string.
fn text() -> String {
    let Some((vm, activity)) = context() else {
        return String::new();
    };
    match vm.attach_current_thread(|env| -> jni::errors::Result<String> {
        // SAFETY: as in `call`.
        let activity = unsafe { env.as_cast_raw::<JObject<'static>>(&activity) }?;
        let value = env
            .call_method(
                &*activity,
                jni_str!("horizonKeyboardText"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .into_object()?;
        if value.is_null() {
            return Ok(String::new());
        }
        let value = env.cast_local::<JString>(value)?;
        value.try_to_string(env)
    }) {
        Ok(text) => text,
        Err(error) => {
            log::warn!("keyboard: cannot read the field: {error}");
            String::new()
        }
    }
}

/// And the keys since the last look: bit 0 Enter, bits 1.. deletes on an empty field.
fn keys() -> i32 {
    let Some((vm, activity)) = context() else {
        return 0;
    };
    match vm.attach_current_thread(|env| -> jni::errors::Result<i32> {
        // SAFETY: as in `call`.
        let activity = unsafe { env.as_cast_raw::<JObject<'static>>(&activity) }?;
        let keys = env
            .call_method(
                &*activity,
                jni_str!("horizonTakeKeyboardKeys"),
                jni_sig!("()I"),
                &[],
            )?
            .i()?;
        Ok(keys)
    }) {
        Ok(keys) => keys,
        Err(error) => {
            log::warn!("keyboard: cannot take the keys: {error}");
            0
        }
    }
}
