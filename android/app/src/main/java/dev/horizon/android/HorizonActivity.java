package dev.horizon.android;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.provider.DocumentsContract;
import android.os.Bundle;
import android.text.Editable;
import android.text.InputType;
import android.text.TextWatcher;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import android.view.inputmethod.InputConnectionWrapper;
import android.view.inputmethod.InputMethodManager;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;
import android.widget.Toast;

/**
 * The native activity, plus the one Android thing the engine cannot reach over JNI:
 * an editable View for the input method to talk to.
 *
 * The game itself is Rust and draws through Vulkan onto the activity's window
 * surface, which NativeActivity takes away from the view system. Nothing here is
 * drawn. The field below is a 1x1 transparent view whose whole job is to take focus,
 * hold what the keyboard commits, and report it: the native side polls its text and
 * the keys that change no text, and says them as edits to the game's focused field.
 */
public class HorizonActivity extends NativeActivity {
    /** The one result this activity asks for: files the player picked to import. */
    private static final int REQUEST_FILE = 0x5c1;
    /** What the last pick was for: skins, mods, resource packs or settings. */
    private String pendingKind = "mod";

    private KeyboardField keyboard;

    /** The launcher's Minecraft install, shown while the engine has no window yet. */
    private AlertDialog installDialog;
    private TextView installText;

    /** The field's text, as the native side last needs to read it. */
    private volatile String keyboardText = "";

    /**
     * Whether edits in the field belong to the game. The native thread clears this
     * before the UI thread has emptied the field, so a poll in between reads the
     * empty string rather than the text of the field the keyboard has just left.
     */
    private volatile boolean keyboardLive = false;

    /** Bit 0: Enter. Bits 1..: deletes over an empty field, which change no text. */
    private int keyboardKeys;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        // NativeActivity loads the engine library and starts android_main in here, so
        // the view has to be added after: setContentView would lose it.
        super.onCreate(savedInstanceState);

        keyboard = new KeyboardField(this);
        keyboard.setImeOptions(
                EditorInfo.IME_ACTION_DONE
                        | EditorInfo.IME_FLAG_NO_EXTRACT_UI
                        | EditorInfo.IME_FLAG_NO_FULLSCREEN);
        // No suggestions: this one field serves commands and chat alike, and an
        // autocorrect has no business rewriting a command.
        keyboard.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS);
        keyboard.setSingleLine(true);
        keyboard.setBackground(null);
        keyboard.setAlpha(0.0f);
        keyboard.setCursorVisible(false);
        keyboard.setSaveEnabled(false);
        keyboard.setImportantForAutofill(View.IMPORTANT_FOR_AUTOFILL_NO);
        keyboard.addTextChangedListener(new TextWatcher() {
            @Override
            public void beforeTextChanged(CharSequence s, int start, int count, int after) {
            }

            @Override
            public void onTextChanged(CharSequence s, int start, int before, int count) {
            }

            @Override
            public void afterTextChanged(Editable s) {
                if (keyboardLive) {
                    keyboardText = s.toString();
                }
            }
        });
        keyboard.setOnEditorActionListener((v, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_DONE
                    && (event == null || event.getAction() == KeyEvent.ACTION_DOWN)) {
                addKeys(1);
                return true;
            }
            return false;
        });
        addContentView(keyboard, new ViewGroup.LayoutParams(1, 1));
    }

    /** The keyboard coming up over a game field, with an empty field of its own. */
    public void horizonShowKeyboard() {
        keyboardLive = false;
        clearKeys();
        runOnUiThread(() -> {
            KeyboardField field = keyboard;
            if (field == null) {
                return;
            }
            field.setText("");
            keyboardText = "";
            keyboardLive = true;
            field.setFocusableInTouchMode(true);
            field.requestFocus();
            InputMethodManager imm = getSystemService(InputMethodManager.class);
            if (imm != null) {
                imm.showSoftInput(field, InputMethodManager.SHOW_IMPLICIT);
            }
        });
    }

    /** The keyboard going away, or the activity leaving the screen. */
    public void horizonHideKeyboard() {
        keyboardLive = false;
        clearKeys();
        runOnUiThread(() -> {
            KeyboardField field = keyboard;
            if (field == null) {
                return;
            }
            field.setText("");
            keyboardText = "";
            InputMethodManager imm = getSystemService(InputMethodManager.class);
            if (imm != null) {
                imm.hideSoftInputFromWindow(field.getWindowToken(), 0);
            }
            field.clearFocus();
        });
    }

    /**
     * The same keyboard, emptied for a field of its own (the next line of a sign).
     * The IME stays up: the player is done with a line, not done typing.
     */
    public void horizonResetKeyboard() {
        keyboardLive = false;
        clearKeys();
        runOnUiThread(() -> {
            KeyboardField field = keyboard;
            if (field == null) {
                return;
            }
            field.setText("");
            keyboardText = "";
            keyboardLive = true;
        });
    }

    /** Everything the field holds now; empty while the keyboard is not up. */
    public String horizonKeyboardText() {
        return keyboardLive ? keyboardText : "";
    }

    /** The keys since the last look: bit 0 Enter, bits 1.. deletes on an empty field. */
    public synchronized int horizonTakeKeyboardKeys() {
        int keys = keyboardKeys;
        keyboardKeys = 0;
        return keys;
    }

    private synchronized void addKeys(int bits) {
        // Enter is a flag and deletes are a count, but they share one int. Adding the
        // whole word meant two Enters arrived as 2, which the native side reads as one
        // delete; OR keeps Enter a flag, and the deletes (bits 1..) still accumulate.
        keyboardKeys |= bits & 1;
        keyboardKeys += bits & ~1;
    }

    private synchronized void clearKeys() {
        keyboardKeys = 0;
    }

    /**
     * The launcher installing Minecraft Java: downloading the pinned client JAR
     * from Mojang, then unpacking it with the seed CI prepared. Called from the
     * native thread before the engine's window exists, so this dialog is the only
     * thing on screen for that whole install.
     */
    public void horizonInstallStart(String message) {
        runOnUiThread(() -> {
            if (installDialog != null) {
                setInstallStatus(message);
                return;
            }
            if (isFinishing() || isDestroyed()) {
                return;
            }
            LinearLayout layout = new LinearLayout(this);
            layout.setOrientation(LinearLayout.VERTICAL);
            int padding = Math.round(20 * getResources().getDisplayMetrics().density);
            layout.setPadding(padding, padding, padding, 0);
            ProgressBar bar = new ProgressBar(this);
            bar.setIndeterminate(true);
            layout.addView(bar);
            TextView text = new TextView(this);
            text.setText(message);
            text.setPadding(0, padding / 2, 0, 0);
            layout.addView(text);
            installText = text;
            AlertDialog dialog = new AlertDialog.Builder(
                    this, android.R.style.Theme_DeviceDefault_Light_Dialog_Alert)
                    .setTitle("Minecraft Java")
                    .setView(layout)
                    .setCancelable(false)
                    .create();
            installDialog = dialog;
            dialog.show();
        });
    }

    /** A progress line from the native side: download bytes, then what is unpacking. */
    public void horizonInstallStatus(String message) {
        runOnUiThread(() -> setInstallStatus(message));
    }

    /** The pack is ready; the engine will take the screen from here. */
    public void horizonInstallDone() {
        runOnUiThread(this::dismissInstallDialog);
    }

    /** Nothing was installed: say why, and let the native card carry on. */
    public void horizonInstallFailed(String message) {
        runOnUiThread(() -> {
            dismissInstallDialog();
            Toast.makeText(this, "Minecraft install failed: " + message, Toast.LENGTH_LONG).show();
        });
    }

    /**
     * Let the player pick files and copy them where the game reads them.
     *
     * `kind` is what the launcher asked for: `skin`, `mod`, `resourcepack` or
     * `settings`. The system picker is the only way to reach a file outside this app's
     * own storage on Android 11+, and the copy is what makes the choice visible to the
     * engine: the document the player picked lives behind a content URI the game's
     * own file reads cannot open.
     */
    public void horizonPickFile(String kind) {
        runOnUiThread(() -> {
            String type = "*/*";
            if ("skin".equals(kind)) {
                type = "image/png";
            } else if ("mod".equals(kind)) {
                type = "*/*";
            }
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            intent.addCategory(Intent.CATEGORY_OPENABLE);
            intent.setType(type);
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
            pendingKind = kind == null ? "mod" : kind;
            try {
                startActivityForResult(intent, REQUEST_FILE);
            } catch (Exception error) {
                Toast.makeText(this, "No file picker on this device", Toast.LENGTH_SHORT).show();
            }
        });
    }

    /**
     * Copy matching files out of the phone's Download folder into `import/<kind>`.
     *
     * A file picker is a second activity, and a picker started from a native activity
     * on a secondary display does not always come up. The Download folder is the one
     * place a phone user already drops things, so this is the path that always works:
     * no dialog, no second activity, just a copy of every file whose extension matches
     * the kind.
     *
     * Returns how many files were copied, or -1 when there is nothing to copy.
     */
    public int horizonImportFromDownload(String kind) {
        String folder = "skin".equals(kind) ? "skins"
                : "resourcepack".equals(kind) ? "resourcepacks"
                : "settings".equals(kind) ? ""
                : "mods";
        String[] extensions;
        if ("skin".equals(kind)) {
            extensions = new String[] { ".png" };
        } else if ("mod".equals(kind)) {
            extensions = new String[] { ".hmod" };
        } else if ("resourcepack".equals(kind)) {
            extensions = new String[] { ".zip" };
        } else {
            extensions = new String[] { ".json", ".txt", ".hmod", ".png", ".zip" };
        }
        java.io.File downloads = new java.io.File(
                android.os.Environment.getExternalStoragePublicDirectory(
                        android.os.Environment.DIRECTORY_DOWNLOADS).getAbsolutePath());
        java.io.File dir = new java.io.File(getExternalFilesDir(null), "import/" + folder);
        if (!dir.isDirectory() && !dir.mkdirs()) {
            return -1;
        }
        java.io.File[] entries = downloads.listFiles();
        if (entries == null) {
            return -1;
        }
        int copied = 0;
        for (java.io.File entry : entries) {
            if (!entry.isFile()) {
                continue;
            }
            String name = entry.getName().toLowerCase();
            boolean wanted = false;
            for (String extension : extensions) {
                if (name.endsWith(extension)) {
                    wanted = true;
                    break;
                }
            }
            if (!wanted) {
                continue;
            }
            try {
                copyFile(entry, new java.io.File(dir, entry.getName()));
                copied++;
            } catch (Exception error) {
                // One unreadable file must not stop the rest.
            }
        }
        if (copied > 0) {
            Toast.makeText(this, "Copied " + copied + " file(s) to import/" + folder,
                    Toast.LENGTH_SHORT).show();
        }
        return copied > 0 ? copied : -1;
    }

    /**
     * Write the game's own files out where the player can reach them.
     *
     * Export is the mirror of import: `kind` names a folder inside `import/`, and every
     * file in it is copied to a folder the file manager can browse. A phone has no
     * "save as" for a folder, so this is a folder, not a dialog.
     */
    public void horizonExportFiles(String kind) {
        runOnUiThread(() -> {
            java.io.File source = new java.io.File(getExternalFilesDir(null),
                    "import/" + (kind == null ? "" : kind));
            if (!source.isDirectory()) {
                Toast.makeText(this, "Nothing to export yet", Toast.LENGTH_SHORT).show();
                return;
            }
            java.io.File target = new java.io.File(getExternalFilesDir(null),
                    "export/" + (kind == null ? "" : kind));
            try {
                copyTree(source, target);
                Toast.makeText(this, "Exported to " + target, Toast.LENGTH_LONG).show();
            } catch (Exception error) {
                Toast.makeText(this, "Export failed: " + error, Toast.LENGTH_LONG).show();
            }
        });
    }

    /** Copy one file, making its parent if the caller's target is nested. */
    private static void copyFile(java.io.File from, java.io.File to) throws java.io.IOException {
        java.io.File parent = to.getParentFile();
        if (parent != null && !parent.isDirectory() && !parent.mkdirs()) {
            throw new java.io.IOException("cannot create " + parent);
        }
        try (java.io.InputStream in = new java.io.FileInputStream(from);
             java.io.OutputStream out = new java.io.FileOutputStream(to)) {
            byte[] buffer = new byte[1 << 16];
            int count;
            while ((count = in.read(buffer)) > 0) {
                out.write(buffer, 0, count);
            }
        }
    }

    private static void copyTree(java.io.File from, java.io.File to) throws java.io.IOException {
        if (!to.isDirectory() && !to.mkdirs()) {
            throw new java.io.IOException("cannot create " + to);
        }
        java.io.File[] entries = from.listFiles();
        if (entries == null) {
            return;
        }
        for (java.io.File entry : entries) {
            java.io.File target = new java.io.File(to, entry.getName());
            if (entry.isDirectory()) {
                copyTree(entry, target);
            } else {
                try (java.io.InputStream in = new java.io.FileInputStream(entry);
                     java.io.OutputStream out = new java.io.FileOutputStream(target)) {
                    byte[] buffer = new byte[1 << 16];
                    int count;
                    while ((count = in.read(buffer)) > 0) {
                        out.write(buffer, 0, count);
                    }
                }
            }
        }
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != REQUEST_FILE || result != RESULT_OK || data == null) {
            return;
        }
        java.util.List<Uri> picked = new java.util.ArrayList<>();
        if (data.getClipData() != null) {
            for (int i = 0; i < data.getClipData().getItemCount(); i++) {
                picked.add(data.getClipData().getItemAt(i).getUri());
            }
        } else if (data.getData() != null) {
            picked.add(data.getData());
        }
        String folder = "skins".equals(pendingKind) ? "skins"
                : "resourcepack".equals(pendingKind) ? "resourcepacks"
                : "settings".equals(pendingKind) ? ""
                : "mods";
        java.io.File dir = new java.io.File(getExternalFilesDir(null), "import/" + folder);
        if (!dir.isDirectory() && !dir.mkdirs()) {
            Toast.makeText(this, "Could not create " + dir, Toast.LENGTH_LONG).show();
            return;
        }
        int copied = 0;
        for (Uri source : picked) {
            String name = displayName(source);
            try (java.io.InputStream in = getContentResolver().openInputStream(source)) {
                if (in == null) {
                    continue;
                }
                java.io.File out = new java.io.File(dir, name);
                try (java.io.OutputStream stream = new java.io.FileOutputStream(out)) {
                    byte[] buffer = new byte[1 << 16];
                    int count;
                    while ((count = in.read(buffer)) > 0) {
                        stream.write(buffer, 0, count);
                    }
                }
                copied++;
            } catch (Exception error) {
                Toast.makeText(this, "Could not read " + name + ": " + error, Toast.LENGTH_LONG).show();
            }
        }
        if (copied > 0) {
            Toast.makeText(this, "Added " + copied + " file(s) to import/" + folder,
                    Toast.LENGTH_SHORT).show();
        }
    }

    /** The file name a content URI carries, with the extension the picker reports. */
    private String displayName(Uri uri) {
        try (android.database.Cursor cursor = getContentResolver().query(uri, null, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int index = cursor.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME);
                if (index >= 0) {
                    String name = cursor.getString(index);
                    if (name != null && !name.isEmpty()) {
                        return name.replace('/', '_');
                    }
                }
            }
        } catch (Exception ignored) {
        }
        String fallback = uri.getLastPathSegment();
        return fallback == null ? "file-" + System.currentTimeMillis() : fallback.replace('/', '_');
    }

    /**
     * Open an https address in whatever the phone uses for browsing.
     *
     * The launcher has links to its own site, and a NativeActivity has no browser of
     * its own: an ACTION_VIEW intent is the only way out of the app that does not
     * involve a web view inside the game's surface.
     */
    public void horizonOpenUrl(String url) {
        if (url == null || !url.startsWith("https://")) {
            return;
        }
        runOnUiThread(() -> {
            try {
                Intent intent = new Intent(Intent.ACTION_VIEW, Uri.parse(url));
                intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
                startActivity(intent);
            } catch (Exception error) {
                Toast.makeText(this, "No browser for " + url, Toast.LENGTH_SHORT).show();
            }
        });
    }

    private void setInstallStatus(String message) {
        if (installText != null) {
            installText.setText(message);
        }
    }

    private void dismissInstallDialog() {
        AlertDialog dialog = installDialog;
        installDialog = null;
        installText = null;
        if (dialog == null) {
            return;
        }
        try {
            dialog.dismiss();
        } catch (IllegalArgumentException gone) {
            // The activity's window was already detached (finishing, or recreated
            // by the system while the install ran); there is nothing left to close.
        }
    }

    @Override
    protected void onDestroy() {
        dismissInstallDialog();
        super.onDestroy();
    }

    /**
     * An EditText that also reports the keys a keyboard sends without changing the
     * text: Enter (the game's own Enter), and a delete over an empty field — which is
     * how a backspace over the game's selected text is seen at all, since the phone's
     * field is empty while the game's is not.
     */
    private final class KeyboardField extends EditText {
        KeyboardField(Context context) {
            super(context);
        }

        @Override
        public InputConnection onCreateInputConnection(EditorInfo outAttrs) {
            InputConnection base = super.onCreateInputConnection(outAttrs);
            if (base == null) {
                return null;
            }
            return new InputConnectionWrapper(base, true) {
                @Override
                public boolean sendKeyEvent(KeyEvent event) {
                    if (event.getAction() == KeyEvent.ACTION_DOWN) {
                        if (event.getKeyCode() == KeyEvent.KEYCODE_ENTER) {
                            addKeys(1);
                            return true;
                        }
                        if (empty() && (event.getKeyCode() == KeyEvent.KEYCODE_DEL
                                || event.getKeyCode() == KeyEvent.KEYCODE_FORWARD_DEL)) {
                            addKeys(2);
                            return true;
                        }
                    }
                    return super.sendKeyEvent(event);
                }

                @Override
                public boolean deleteSurroundingText(int beforeLength, int afterLength) {
                    if (empty() && beforeLength > 0) {
                        addKeys(2);
                        return true;
                    }
                    return super.deleteSurroundingText(beforeLength, afterLength);
                }

                @Override
                public boolean deleteSurroundingTextInCodePoints(int beforeLength, int afterLength) {
                    if (empty() && beforeLength > 0) {
                        addKeys(2);
                        return true;
                    }
                    return super.deleteSurroundingTextInCodePoints(beforeLength, afterLength);
                }

                private boolean empty() {
                    return keyboard != null && keyboard.length() == 0;
                }
            };
        }
    }
}
