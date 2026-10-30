//! The in-game menu: the tab at the top edge, and the list it slides out.
//!
//! This is the launcher's own menu, not the game's. It is the only way into the pad's
//! arrange mode, the frame line, and the quick settings, without a keyboard and
//! without a file manager.
//!
//! The shape is the one a player already knows from other launchers: a small tab
//! hanging from the top edge with a gear on it, and a list that slides in from the
//! right when it is tapped. The tab is deliberately faint — it is over the game, and
//! a menu handle that is easy to see is a menu handle that hides the world.

use imgui::{Condition, Ui, WindowFlags};

use crate::Rect;

/// What the menu is for. The list changes when the pad is being arranged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuMode {
    /// The ordinary in-game menu.
    Game,
    /// The pad is being arranged: add, save, leave.
    Arrange,
}

/// What the player picked from the menu this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuChoice {
    Nothing,
    /// Show or hide the frame line.
    ToggleFrameLine,
    /// Open the pad's arrange mode.
    ArrangeControls,
    /// Leave the arrange mode, saving what was changed.
    SaveLayout,
    /// Put every button back where the layout file has it.
    ResetLayout,
    /// Leave the arrange mode.
    CloseEditor,
    /// Back to the launcher.
    Quit,
}

/// The gear tab's own rectangle, in glass pixels: 40x20 dp at the top edge, three
/// quarters of the way across.
pub fn tab_rect(width: f32, density: f32) -> Rect {
    let w = 40.0 * density;
    let h = 20.0 * density;
    // The tab is laid out centred and then shifted a quarter of the screen to the
    // right, which puts it at three quarters of the width.
    Rect {
        x: (width - w) / 2.0 + width * 0.25,
        y: 0.0,
        width: w,
        height: h,
    }
}

/// Draw the tab and, when it is open, the list.
///
/// `open` is the drawer's own state and is toggled by the tab. The return value is
/// what the player picked, which is `Nothing` on the frames where the list is only
/// being drawn.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ui: &Ui,
    open: &mut bool,
    mode: MenuMode,
    width: f32,
    height: f32,
    density: f32,
    frame_line: bool,
    fps: f32,
    memory: Option<f64>,
    mouse_speed: &mut f32,
    mouse_scale: &mut f32,
) -> MenuChoice {
    let mut choice = MenuChoice::Nothing;
    let tab = tab_rect(width, density);

    // The tab: a black half-circle hanging from the top edge, a white gear on it, at a
    // third of the screen's alpha so it does not fight the world behind it.
    {
        let draw = ui.get_window_draw_list();
        let pad = 8.0 * density;
        let center = [tab.x + tab.width / 2.0, 0.0];
        // The visible half of the circle: the bottom one, below the top edge.
        draw.add_circle(center, tab.height * 0.95, [0.0, 0.0, 0.0, 0.33])
            .filled(true)
            .num_segments(24)
            .build();
        // A gear, drawn as a ring with teeth rather than a texture: no asset, and it
        // is crisp at any size.
        let r = (tab.height - pad) * 0.32;
        let gy = tab.height * 0.55;
        let teeth = 8;
        for tooth in 0..teeth {
            let angle = tooth as f32 * std::f32::consts::TAU / teeth as f32;
            let (sin, cos) = angle.sin_cos();
            draw.add_line(
                [center[0] + cos * r * 0.8, gy + sin * r * 0.8],
                [center[0] + cos * r * 1.35, gy + sin * r * 1.35],
                [1.0, 1.0, 1.0, 0.75],
            )
            .thickness(2.0 * density)
            .build();
        }
        draw.add_circle([center[0], gy], r, [1.0, 1.0, 1.0, 0.75])
            .filled(false)
            .thickness(2.0 * density)
            .num_segments(20)
            .build();
    }
    // The tab is a real hit target, invisible to imgui's own windows.
    ui.set_cursor_screen_pos([tab.x, 0.0]);
    if ui.invisible_button("##menu_tab", [tab.width, tab.height]) {
        *open = !*open;
    }

    if !*open {
        return choice;
    }

    // The list: a right-hand column, as wide as a thumb is long, full height.
    let panel_w = 200.0 * density;
    let x = width - panel_w;
    let _ = ui
        .window("##in_game_menu")
        .flags(
            WindowFlags::NO_TITLE_BAR
                | WindowFlags::NO_RESIZE
                | WindowFlags::NO_MOVE
                | WindowFlags::NO_COLLAPSE
                | WindowFlags::NO_SCROLLBAR
                | WindowFlags::NO_SCROLL_WITH_MOUSE
                | WindowFlags::NO_SAVED_SETTINGS
                | WindowFlags::NO_FOCUS_ON_APPEARING,
        )
        .position([x, 0.0], Condition::Always)
        .size([panel_w, height], Condition::Always)
        .build(|| {
            // The scrim: the rest of the screen dims, so the list reads as a layer
            // above the world rather than as part of it.
            {
                let draw = ui.get_window_draw_list();
                draw.add_rect([0.0, 0.0], [width, height], [0.0, 0.0, 0.0, 0.35])
                    .filled(true)
                    .build();
                draw.add_rect([x, 0.0], [width, height], [0.09, 0.09, 0.11, 0.98])
                    .filled(true)
                    .build();
                draw.add_line([x, 0.0], [x, height], [1.0, 1.0, 1.0, 0.16])
                    .thickness(1.0 * density)
                    .build();
            }
            ui.set_cursor_screen_pos([x + 16.0 * density, 16.0 * density]);
            ui.text("Horizon");
            ui.set_cursor_screen_pos([x + 16.0 * density, 40.0 * density]);
            ui.separator();
            // The frame line the engine measured: the two numbers worth glancing at.
            ui.text(format!("{fps:.0} fps"));
            match memory {
                Some(mib) => ui.text(format!("{mib:.0} MB")),
                None => ui.text("memory n/a"),
            }
            ui.separator();
            let row = [panel_w - 32.0 * density, 34.0 * density];
            let mut item = |label: &str| {
                ui.button_with_size(label, row)
            };
            match mode {
                MenuMode::Game => {
                    if item(if frame_line { "Hide the frame line" } else { "Show the frame line" }) {
                        choice = MenuChoice::ToggleFrameLine;
                    }
                    if item("Customise controls") {
                        choice = MenuChoice::ArrangeControls;
                    }
                    ui.separator();
                    ui.text("Mouse");
                    // The cursor's own two numbers. A thumb that sweeps the glass and a
                    // pointer that crosses the screen in one flick cannot be aimed, and
                    // a cursor the size of a pinhead cannot be found.
                    ui.set_next_item_width(panel_w - 32.0 * density);
                    ui.slider("Speed", 0.25, 3.0, mouse_speed);
                    ui.set_next_item_width(panel_w - 32.0 * density);
                    ui.slider("Size", 0.5, 3.0, mouse_scale);
                    ui.separator();
                    if item("Back to the launcher") {
                        choice = MenuChoice::Quit;
                    }
                }
                MenuMode::Arrange => {
                    if item("Save layout") {
                        choice = MenuChoice::SaveLayout;
                    }
                    if item("Reset layout") {
                        choice = MenuChoice::ResetLayout;
                    }
                    if item("Leave editor") {
                        choice = MenuChoice::CloseEditor;
                    }
                }
            }
        });
    // A tap outside the list closes it, the way a drawer does.
    if ui.io().mouse_clicked[0] && !ui.is_mouse_pos_valid([width / 2.0, height / 2.0]) {
        *open = false;
    }
    choice
}
