package dev.horizon.android;

import android.app.NativeActivity;
import android.content.Context;
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
    private KeyboardField keyboard;

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
        keyboardKeys += bits;
    }

    private synchronized void clearKeys() {
        keyboardKeys = 0;
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
