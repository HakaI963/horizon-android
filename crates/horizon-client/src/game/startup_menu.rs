//! The interactive title before world resources exist. Uses only the menu
//! atlas/panorama; transfers the actual screen and queued entry to Game.
use super::*;
use horizon_client::{
    clipboard::{self, AppendSelection, SystemClipboard},
    options_ui::{Event, OptionsUi, Page},
};
use std::path::PathBuf;
use winit::{event::KeyEvent, keyboard::ModifiersState};

#[cfg(target_os = "android")]
use horizon_overlay::{
    LauncherAccount, LauncherEvent, LauncherMod, LauncherModel, LauncherPage, LauncherRow,
};

/// Where a Microsoft sign-in has got to. The flow itself is `auth`/`account`; this is
/// only what the launcher needs to draw while it runs.
#[cfg(target_os = "android")]
#[derive(Clone, Debug)]
pub(crate) enum SignInState {
    /// Waiting for the player to enter the code at the Microsoft page.
    Waiting(String),
    /// Signed in as this name.
    Done(String),
    /// Stopped, with the reason.
    Failed(String),
}

/// Horizon's own mod registry, where a mod's page lives. The launcher shows the same
/// registry in its Mods page; this is the link out to the site for everything else.
#[cfg(target_os = "android")]
const MODS_SITE: &str = "https://playhorizon.dev/mods";

#[derive(Default)]
pub(super) struct StartupMenu {
    title: Option<Title>,
    options: OptionsUi,
    options_open: bool,
    /// The launcher page shown at boot on a phone, or `None` once the player has
    /// gone into the game's own menus. The desktop boots straight to the title.
    #[cfg(target_os = "android")]
    launcher_page: Option<LauncherPage>,
    /// What the phone's keyboard has given the launcher's name field so far.
    #[cfg(target_os = "android")]
    launcher_draft: String,
    /// The world list the launcher draws, read once instead of every frame: each
    /// entry costs a `level.dat` read and an NBT parse, and the launcher redraws at
    /// the phone's refresh rate for as long as it is up.
    #[cfg(target_os = "android")]
    launcher_worlds: Option<Vec<horizon_client::title::WorldEntry>>,
    /// What the last export or import said, shown under the settings page.
    #[cfg(target_os = "android")]
    launcher_notice: Option<String>,
    /// The credits from Credits & Attribution, and their last frame.
    win: Option<(horizon_client::win_screen::WinScreen, Instant)>,
    root: PathBuf,
    cursor: (f32, f32),
    pointer: (f64, f64),
    modifiers: ModifiersState,
    physical_keys: HashSet<KeyCode>,
    dragging: bool,
    clipboard: SystemClipboard,
    selection: AppendSelection,
    pub pending: Option<PendingWorldEntry>,
    pub reload_packs: bool,
    pub perf: horizon_client::telemetry::Session,
}

impl StartupMenu {
    pub fn new(options: &Options, pack_error: Option<&str>) -> Result<Self> {
        let pending = if let Some(address) = &options.server {
            Some(PendingWorldEntry::Server {
                address: address.clone(),
                name: options.player_name(),
            })
        } else {
            options
                .world
                .as_ref()
                .map(|folder| PendingWorldEntry::Local {
                    folder: folder.clone(),
                    name: None,
                    seed: Some(options.seed),
                    creative: options.creative,
                    create: None,
                })
        };
        let mut menu = Self {
            pending,
            ..Default::default()
        };
        menu.root = options.resources.clone();
        // From the first frame, not only once the artwork arrives: the finished load
        // asks whether the launcher is still up, and an answer of "not yet" would let
        // the game take the screen out from under it.
        #[cfg(target_os = "android")]
        {
            menu.launcher_page = Some(LauncherPage::Play);
        }
        if let Some(error) = pack_error {
            menu.options.open_at(false, Page::Packs);
            menu.options.resource_error = Some(error.to_owned());
            menu.options_open = true;
        }
        if menu.pending.is_some() {
            menu.perf.world_opening();
        }
        Ok(menu)
    }

    pub(super) fn set_root(&mut self, root: PathBuf, settings: &Settings) {
        horizon_client::ui::load_metadata(&root);
        horizon_client::lang::set(horizon_client::lang::Language::load(
            &[&root],
            &settings.language,
        ));
        self.root = root;
        if self.title.is_none() {
            let mut title = Title::new(
                std::env::current_dir().unwrap_or_default().join("saves"),
                &self.root,
            );
            title.last_server = settings.file.string("lastServer");
            self.title = Some(title);
        }
        // The front page is armed once, in `new`. This runs again for every reload
        // (a resource pack changed from Options, the atlas rebuilt), and re-arming it
        // here walked the player back onto the launcher from wherever they had got to.
    }

    /// Leave the game's own menus and go back to the launcher's front page.
    ///
    /// The phone boots into the launcher, so it is the home screen of this program:
    /// the title's "Quit Game" means leaving Minecraft, not leaving Horizon. The zoom
    /// back out is the launcher's own opening, played in reverse, which the overlay
    /// runs when it is shown again.
    #[cfg(target_os = "android")]
    pub(super) fn return_to_launcher(&mut self) {
        self.options_open = false;
        if let Some(title) = &mut self.title {
            title.mode = horizon_client::title::Mode::Launcher;
        }
        self.launcher_page = Some(LauncherPage::Play);
        self.launcher_worlds = None;
        self.launcher_notice = None;
        log::info!("launcher: back to the front page from the game's own menu");
    }

    /// Whether the launcher's front page is still on the screen. The engine may finish
    /// loading behind it — it just must not take the screen over until the player leaves.
    #[cfg(target_os = "android")]
    pub(super) fn launcher_up(&self) -> bool {
        self.launcher_page.is_some()
    }

    pub(super) fn handoff(&mut self, game: &mut Game<'_>) {
        game.title = self.title.take();
        game.pending_entry = self.pending.take();
        game.cursor = self.cursor;
        game.modifiers = self.modifiers;
        game.physical_keys = std::mem::take(&mut self.physical_keys);
        game.clipboard = std::mem::take(&mut self.clipboard);
        game.text_selection = std::mem::take(&mut self.selection);
        game.perf = std::mem::take(&mut self.perf);
        // Preserve an existing resource-error screen opened by start_game.
        if self.options_open {
            let resource_error = game.ui.options.resource_error.take();
            game.ui.options = std::mem::take(&mut self.options);
            game.ui.options.resource_error =
                game.ui.options.resource_error.take().or(resource_error);
            game.screen = Screen::Options;
            game.options_from_title = true;
            game.dragging = self.dragging;
        }
    }

    fn queue_entry(&mut self, action: Action, player_name: String) {
        let entry = match action {
            Action::Play { folder } => PendingWorldEntry::Local {
                folder,
                name: None,
                seed: None,
                creative: false,
                create: None,
            },
            Action::Create {
                folder,
                name,
                seed,
                creative,
                options,
            } => PendingWorldEntry::Local {
                folder,
                name: Some(name),
                seed,
                creative,
                create: Some(Box::new(options)),
            },
            Action::Connect { address } => PendingWorldEntry::Server {
                address,
                name: player_name,
            },
            _ => return,
        };
        self.pending = Some(entry);
        self.perf.world_opening();
    }

    fn shortcut(&mut self, event: &KeyEvent) -> Option<clipboard::EditShortcut> {
        let edit = if cfg!(target_os = "macos") {
            self.modifiers.super_key()
        } else {
            self.modifiers.control_key()
        };
        clipboard::edit_shortcut(
            clipboard::shortcut_letter(&event.logical_key, event.physical_key),
            edit,
            self.modifiers.shift_key(),
            self.modifiers.alt_key(),
        )
    }

    /// The title's own keys. Returns the action an Enter submits, so the caller can
    /// carry it out exactly as a click would: `title_key` has no event loop of its own.
    fn title_key(&mut self, event: &KeyEvent) -> Option<Action> {
        let shortcut = self.shortcut(event);
        let Some(title) = &mut self.title else {
            return None;
        };
        if let Some(shortcut) = shortcut {
            if title.has_caret_field() {
                title.field_shortcut(shortcut, &mut self.clipboard);
            } else if let Some((id, value, max)) = title.focused_field()
                && self
                    .selection
                    .apply(id, value, shortcut, &mut self.clipboard, |v, t| {
                        clipboard::append_limited(v, t, max)
                    })
            {
                title.search_changed();
            }
            return None;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => title.escape(),
            PhysicalKey::Code(KeyCode::Tab) => title.tab(),
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                let action = title.confirm_edit();
                if !matches!(action, Action::None) {
                    return Some(action);
                }
            }
            PhysicalKey::Code(
                key @ (KeyCode::Backspace
                | KeyCode::Delete
                | KeyCode::ArrowLeft
                | KeyCode::ArrowRight
                | KeyCode::Home
                | KeyCode::End),
            ) if title.has_caret_field() => {
                title.field_key(
                    key,
                    self.modifiers.control_key(),
                    self.modifiers.shift_key(),
                );
            }
            _ => {
                let cleared = !title.has_caret_field()
                    && title
                        .focused_field()
                        .is_some_and(|(id, value, _)| self.selection.take(id, value));
                if cleared {
                    title.search_changed();
                }
                if event.physical_key == PhysicalKey::Code(KeyCode::Backspace) {
                    if !cleared {
                        title.backspace();
                    }
                } else if let Some(text) = &event.text {
                    title.type_text(text);
                }
            }
        }
        None
    }

    /// Whether a text field has the focus, which on a phone is the only reason the
    /// keyboard should cover the menu. The main title's list and an options page
    /// between searches have none.
    fn typing(&mut self) -> bool {
        if self.win.is_some() || self.pending.is_some() {
            return false;
        }
        if self.options_open {
            return self.options.typing();
        }
        self.title
            .as_mut()
            .is_some_and(|title| title.focused_field().is_some())
    }

    /// Select All in the title's focused field. The phone's field is empty, so this
    /// is what makes the next word replace what the game already has.
    #[cfg(target_os = "android")]
    fn ime_select_all(&mut self) {
        let Some(title) = self.title.as_mut() else {
            return;
        };
        if title.has_caret_field() {
            title.field_shortcut(clipboard::EditShortcut::SelectAll, &mut self.clipboard);
        } else if let Some((id, value, max)) = title.focused_field() {
            let changed = self.selection.apply(
                id,
                value,
                clipboard::EditShortcut::SelectAll,
                &mut self.clipboard,
                |v, t| clipboard::append_limited(v, t, max),
            );
            if changed {
                title.search_changed();
            }
        }
    }
}

/// Each generation has its own channel and progress. Pack changes wait for the
/// old tail before starting another generation, including cache/global writes.
pub(super) fn spawn_loader(
    options: &Options,
    settings: &Settings,
    vanilla_root: &std::path::Path,
    started: Instant,
    progress: &Progress,
) -> Result<(Receiver<LoaderMsg>, JoinHandle<()>)> {
    let (sender, receiver) = std::sync::mpsc::channel();
    let settings = settings.clone();
    let vanilla_root = vanilla_root.to_path_buf();
    let dev_mods = options.dev_mods.clone();
    let progress = progress.clone();
    let handle = std::thread::Builder::new()
        .name("resource-loader".into())
        .spawn(move || {
            let mut pack_error = None;
            let root = packs::startup_root(&vanilla_root, &settings.resource_packs, &[])
                .unwrap_or_else(|error| {
                    pack_error = Some(error.to_string());
                    vanilla_root.clone()
                });
            let root = mods::content_root(&root, &settings.mods.enabled, &dev_mods);
            let result = load_resources(
                root,
                &vanilla_root,
                &settings,
                started,
                true,
                &progress,
                &mut |gui, root| {
                    let _ = sender.send(LoaderMsg::Ui(gui, root.to_path_buf()));
                },
            )
            .map(|mut loaded| {
                loaded.pack_error = loaded.pack_error.or(pack_error);
                loaded
            })
            .map_err(|error| error.to_string());
            let _ = sender.send(LoaderMsg::Done(result));
        })?;
    Ok((receiver, handle))
}

impl Loading {
    pub(super) fn poll_menu_resources(&mut self) -> Result<()> {
        if self.loaded.is_none() {
            loop {
                match self.receiver.try_recv() {
                    Ok(LoaderMsg::Ui(gui, root)) => self.set_ui(gui, root),
                    Ok(LoaderMsg::Done(result)) => {
                        self.loaded = Some(result);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if let Some(handle) = self.handle.take() {
                            let _ = handle.join();
                        }
                        return Err("resource loader stopped before finishing".into());
                    }
                }
            }
        }
        self.restart_menu_resources()
    }
    pub(super) fn apply_menu_settings(&mut self) {
        if let Some(renderer) = &mut self.renderer {
            self.settings
                .clamp_gui_scale([renderer.config.width as i32, renderer.config.height as i32]);
            renderer.gamma = self.settings.gamma;
            renderer.set_present_mode(self.settings.vsync, self.settings.horizon.present_mode);
        }
        horizon_client::hud::GUI_SCALE.store(self.settings.gui_scale, Ordering::Relaxed);
        if let Some(gui) = &mut self.gui {
            gui.atlas.font.uniform = self.settings.force_unicode;
            gui.atlas.font.japanese = self.settings.japanese_glyphs;
        }
        if let Some(window) = &self.window
            && let Some(want) = self.fullscreen.request(self.settings.fullscreen)
        {
            window.set_fullscreen(want.then_some(winit::window::Fullscreen::Borderless(None)));
        }
        self.update_menu_cursor();
    }

    fn update_menu_cursor(&mut self) {
        let batch = HudBatch::new(self.menu_size());
        self.menu.cursor = (
            self.menu.pointer.0 as f32 / batch.scale,
            self.menu.pointer.1 as f32 / batch.scale,
        );
    }

    fn menu_size(&self) -> [u32; 2] {
        self.renderer
            .as_ref()
            .map_or([1280, 720], |r| [r.config.width, r.config.height])
    }

    fn save_menu_address(&mut self) {
        if self
            .menu
            .title
            .as_ref()
            .is_some_and(|title| title.last_server != self.settings.file.string("lastServer"))
        {
            self.save_menu_settings();
        }
    }

    fn save_menu_settings(&mut self) {
        if let Some(title) = &self.menu.title {
            self.settings.file.set(
                "lastServer",
                horizon_client::options_file::V::Str(title.last_server.clone()),
            );
        }
        self.settings.save(&Game::options_path());
    }

    fn open_menu_options(&mut self, page: Page) {
        self.menu.options.open_at(false, page);
        self.menu.options.languages = horizon_client::lang::available(&[&self.menu.root]);
        self.menu.options.pack_infos = packs::pack_infos(&self.vanilla_root);
        self.menu.options.pack_selection = self.settings.resource_packs.clone();
        self.menu.options.pack_screen = None;
        self.menu.options_open = true;
    }

    fn menu_option_events(&mut self, events: Vec<Event>) {
        for event in events {
            match event {
                Event::Changed => {
                    self.save_menu_settings();
                    self.apply_menu_settings();
                }
                Event::Close if self.menu.options.resource_error.is_some() => {}
                Event::Close => {
                    self.menu.options_open = false;
                    self.menu.dragging = false;
                    self.save_menu_settings();
                }
                Event::Language(code) => {
                    horizon_client::lang::set(horizon_client::lang::Language::load(
                        &[&self.menu.root],
                        &code,
                    ));
                    self.save_menu_settings();
                }
                Event::Packs(_) => {
                    self.save_menu_settings();
                    self.menu.reload_packs = true;
                }
                Event::OpenPackFolder => {
                    let dir = packs::packs_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    packs::open_folder(&dir);
                }
                Event::OpenModFolder => {
                    let dir = Game::options_path().with_file_name(horizon_client::mod_list::DIR);
                    let _ = std::fs::create_dir_all(&dir);
                    packs::open_folder(&dir);
                }
                Event::Credits => {
                    if let Some(gui) = &self.gui {
                        let name = self.options.as_ref().map_or_else(
                            || horizon_client::offline_name::generate(0),
                            Options::player_name,
                        );
                        let screen = horizon_client::win_screen::WinScreen::new(
                            false,
                            &self.menu.root,
                            &name,
                            &gui.atlas.font,
                        );
                        self.menu.win = Some((screen, Instant::now()));
                    }
                }
                // The startup menu runs before the game opens a control socket.
                Event::ButtonClick | Event::CopyAgentDetails => {}
            }
        }
    }

    fn menu_options_click(&mut self, drag: bool, right: bool) {
        let size = self.menu_size();
        let batch = HudBatch::new(size);
        let Some(gui) = &self.gui else { return };
        let events = self.menu.options.click(
            &mut self.settings,
            &gui.atlas,
            self.menu.cursor.0,
            self.menu.cursor.1,
            (batch.width, batch.height),
            [size[0] as i32, size[1] as i32],
            drag,
            right,
        );
        self.menu_option_events(events);
    }

    fn menu_action(&mut self, event_loop: &ActiveEventLoop, action: Action) {
        match action {
            // On a phone the launcher is the home screen, so "Quit Game" from the
            // game's own menu means "back to the launcher", not "end the process": the
            // player asked to leave Minecraft, not to close Horizon. The desktop has no
            // launcher behind the title, so there it still exits.
            Action::Quit => {
                #[cfg(target_os = "android")]
                {
                    self.menu.return_to_launcher();
                }
                #[cfg(not(target_os = "android"))]
                event_loop.exit()
            }
            // The game's own Mods button is the launcher's Mods page, which is where
            // installing one lives. The title is the launcher's own front page here, so
            // this is a page change, not a screen of its own.
            Action::Mods => {
                #[cfg(target_os = "android")]
                {
                    self.menu.launcher_page = Some(LauncherPage::Mods);
                    horizon_overlay::with(|o| o.set_launcher_page(LauncherPage::Mods));
                }
            }
            Action::Options => self.open_menu_options(Page::Main),
            Action::Accessibility => self.open_menu_options(Page::Accessibility),
            Action::Language => self.open_menu_options(Page::Language),
            action => {
                let name = self
                    .options
                    .as_ref()
                    .map_or_else(|| "Player".into(), |o| o.player_name());
                self.menu.queue_entry(action, name);
            }
        }
        self.save_menu_address();
    }

    /// Gather what the launcher shows from the engine, hand it to the overlay, and
    /// switch the overlay between the launcher and the touch HUD. A no-op on the
    /// desktop, which has no launcher.
    #[cfg(target_os = "android")]
    fn sync_launcher(&mut self, event_loop: &ActiveEventLoop) {
        // Take last frame's clicks before reading the page, so this frame's model
        // already reflects them.
        self.handle_launcher_events(event_loop);
        // A Microsoft sign-in runs on a worker; its answer arrives here.
        self.poll_sign_in();
        if self.menu.launcher_page.is_none() {
            // A Play transition keeps the launcher on the glass until the zoom has
            // played out: the world is loading behind it, and clearing the page at the
            // tap would leave a frozen frame where the animation is.
            if let Some(until) = self.zoom_until {
                if std::time::Instant::now() < until {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    return;
                }
                self.zoom_until = None;
            }
            let _ = horizon_overlay::with(|o| o.show_launcher(false));
            return;
        }
        // The launcher owns the page: the sidebar writes it into its own model the
        // frame the click lands, and the list below is built from that, so the two
        // can never disagree even if the event reaches here a frame later.
        let page = horizon_overlay::with(|o| {
            o.show_launcher(true);
            // The engine has the title panorama behind this window, and the front page
            // is drawn over it rather than painting it out.
            o.set_panorama(true);
            o.launcher_page()
        })
        .unwrap_or(LauncherPage::Play);
        // imgui needs a second frame to finish a press: the finger goes down in one
        // frame and lifts in the next. The loading surface otherwise draws on demand.
        event_loop.set_control_flow(ControlFlow::Poll);
        let saves = self
            .menu
            .title
            .as_ref()
            .map(|t| t.saves.clone())
            .unwrap_or_default();
        // Listing a world reads and NBT-parses its `level.dat`, and this runs on every
        // frame the launcher is up. The cache is rebuilt on the pages that show it.
        let needs_worlds = matches!(
            page,
            LauncherPage::Play | LauncherPage::Installations | LauncherPage::Multiplayer
        );
        if needs_worlds && self.menu.launcher_worlds.is_none() {
            self.menu.launcher_worlds = Some(horizon_client::title::list_worlds(&saves));
        }
        let worlds = self.menu.launcher_worlds.clone().unwrap_or_default();
        // The sound pack fills in on its own thread and has no way to reach this one,
        // so it leaves a counter behind for the card to read.
        let sounds = horizon_client::sound_progress::snapshot();
        let version = horizon_core::VANILLA_VERSION.to_string();
        let account = self
            .options
            .as_ref()
            .map_or_else(|| "Player".into(), |o| o.player_name());
        // The skins the page offers, and which one is chosen. Reading the folder is a
        // directory listing, and this runs on every frame the page is up, so it is
        // only done for the page that shows it.
        let (skins, chosen_skin) = if page == LauncherPage::Skins {
            let dir = horizon_client::gfx::import_dir().join("skins");
            (
                horizon_client::skin_choice::available(&dir),
                horizon_client::skin_choice::load(&dir),
            )
        } else {
            (Vec::new(), None)
        };
        let accounts = self.launcher_accounts();
        let controls_preset = self.launcher_controls_preset();
        // Turn the Skins model before the model is built, so the frame drawn is the
        // frame the angle belongs to. A still figure reads as a drawing, and the point
        // of the page is that this is the player.
        if page == LauncherPage::Skins {
            let now = std::time::Instant::now();
            let delta = now.duration_since(self.skin_turned).as_secs_f32();
            self.skin_turned = now;
            self.skin_yaw = (self.skin_yaw + delta * 0.45) % std::f32::consts::TAU;
        }
        // A sign-in that is waiting on the browser says so above the account list; a
        // finished one has already moved the notice to "Signed in as ...".
        let sign_in_line = match &self.sign_in {
            Some(SignInState::Waiting(code)) => Some(format!(
                "Waiting for Microsoft: type {code} at microsoft.com/link"
            )),
            Some(SignInState::Failed(error)) => Some(error.clone()),
            Some(SignInState::Done(_)) | None => None,
        };
        let mut model = LauncherModel {
            page,
            version: version.clone(),
            account,
            draft: self.menu.launcher_draft.clone(),
            facts: vec![
                ("Minecraft".into(), version.clone()),
                ("Engine".into(), "Horizon · Rust".into()),
                ("Platform".into(), "Android · native".into()),
            ],
            skins,
            chosen_skin: chosen_skin.clone(),
            skin_yaw: self.skin_yaw,
            accounts,
            controls_preset,
            hosting: self.hosting_address(),
            news: news_stories(),
            patches: patch_stories(),
            ..Default::default()
        };
        // The Skins page needs the chosen PNG on the GPU to draw the player out of its
        // own pixels. This uploads only when the file changed, so it costs nothing on
        // the frames that follow.
        if page == LauncherPage::Skins {
            let path = chosen_skin
                .as_ref()
                .map(|name| horizon_client::gfx::import_dir().join("skins").join(name))
                .filter(|path| path.is_file());
            self.launcher_skin = path.clone();
            model.skin_size = path
                .as_ref()
                .and_then(|path| png_size(path))
                .unwrap_or([0, 0]);
            if path != self.uploaded_skin
                && let Some(r) = &self.renderer
            {
                horizon_overlay::with(|o| o.set_launcher_skin(&r.device, &r.queue, path.as_deref()));
                self.uploaded_skin = path;
            }
        }
        if page == LauncherPage::Mods {
            let import = horizon_client::gfx::import_dir();
            // The notice is read before the registry is borrowed: `mod_registry` takes
            // `&mut self`, so nothing else on `self` can be touched while it is held.
            let notice = self.menu.launcher_notice.clone();
            let registry = self.mod_registry();
            // A fetch failure is the more urgent line: the install notice describes
            // something that already worked, and a registry that could not be reached
            // is why the list is empty. So the error wins, then the notice, then the
            // count.
            model.mods_loading = registry.is_none();
            model.status = match registry {
                None => String::new(),
                Some(registry) if registry.failed => registry.status.clone(),
                Some(registry) => notice.unwrap_or_else(|| registry.status.clone()),
            };
            model.mods_failed = registry.is_some_and(|registry| registry.failed);
            model.installed_mods = horizon_client::mod_registry::installed(&import);
            model.mods_site = MODS_SITE.to_string();
            model.mods = registry
                .map(|registry| {
                    registry
                        .entries
                        .iter()
                        .map(|entry| LauncherMod {
                            id: entry.id.clone(),
                            name: entry.name.clone(),
                            summary: entry.summary.clone(),
                            version: entry.version.clone(),
                            file: entry
                                .url
                                .rsplit('/')
                                .next()
                                .unwrap_or_default()
                                .to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
        match page {
            LauncherPage::Play => {
                model.playable = true;
                model.instance = "Default".into();
                if let Some((line, fraction)) = sounds {
                    model.status = line;
                    model.progress = Some(fraction);
                }
            }
            LauncherPage::Installations => {
                // Versions, and only versions. The worlds under them used to be listed
                // here as well, which made a page called Installations into a second
                // world list; a world belongs on Play, not beside the thing that
                // installed the game.
                model.playable = true;
                model.instance = "Default".into();
                model.rows = vec![LauncherRow {
                    id: 0,
                    title: format!("Minecraft {version}"),
                    subtitle: "Horizon · the pack this build was made for".into(),
                    badge: "installed".into(),
                    selected: true,
                    enabled: true,
                }];
            }
            LauncherPage::Multiplayer => {
                model.instance = "Default".into();
                model.rows = worlds
                    .iter()
                    .enumerate()
                    .map(|(i, w)| LauncherRow {
                        id: i as u32,
                        title: w.name.clone(),
                        subtitle: w.version.clone(),
                        badge: if w.creative { "Creative".into() } else { String::new() },
                        selected: false,
                        enabled: true,
                    })
                    .collect();
            }
            LauncherPage::Files => {
                model.rows = files_rows();
            }
            LauncherPage::Settings => {
                model.settings = self.launcher_settings();
                if let Some(notice) = self.menu.launcher_notice.clone() {
                    model.status = notice;
                }
            }
            LauncherPage::Profile => {
                // The sign-in's own state first: it is the one line that changes while
                // the player is looking at the page.
                model.status = sign_in_line
                    .or_else(|| self.menu.launcher_notice.clone())
                    .unwrap_or_default();
            }
            _ => {}
        }
        let _ = horizon_overlay::with(|o| o.set_launcher_model(model));
        // Ask for the next frame by name. A press is only a press when imgui gets the
        // frame after the lift to see the release, and the report above is only worth
        // drawing once the page the finger chose has arrived; a loading surface that
        // waits for events would sit on the frame before both.
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Carry out what the launcher asked for. Play and Settings leave the launcher
    /// and hand over to the game's own screens; a world row launches straight in.
    #[cfg(target_os = "android")]
    fn handle_launcher_events(&mut self, event_loop: &ActiveEventLoop) {
        let events = horizon_overlay::with(|o| o.take_launcher_events()).unwrap_or_default();
        for event in events {
            log::info!("launcher: {event:?}");
            match event {
                LauncherEvent::Goto(page) => {
                    // The overlay owns which page is on the glass, and it is the only
                    // place both the sidebar and these buttons can agree. Setting it
                    // here as well as on the model is what makes FILES, MULTIPLAYER,
                    // Settings and Back actually move: before, the sidebar was the only
                    // writer, and every other navigation was overwritten by the next
                    // model report.
                    self.menu.launcher_page = Some(page);
                    horizon_overlay::with(|o| o.set_launcher_page(page));
                    // A page that lists worlds re-reads them: a world may have been
                    // made, renamed or deleted since the list was last built.
                    if matches!(
                        page,
                        LauncherPage::Play | LauncherPage::Installations | LauncherPage::Multiplayer
                    ) {
                        self.menu.launcher_worlds = None;
                    }
                }
                LauncherEvent::Play => {
                    // The zoom runs on the glass while the game's own title screen
                    // loads behind it, so the one-to-three seconds a first frame costs
                    // are covered by an animation instead of a still frame. The page is
                    // not cleared until the zoom is spent; `sync_launcher` holds the
                    // launcher up until `launcher_zoom` reports it is done.
                    horizon_overlay::with(|o| o.begin_launcher_zoom());
                    self.zoom_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(560));
                    // The page closes now so the game takes over, but the launcher keeps
                    // drawing for the length of the zoom (see `sync_launcher`).
                    self.menu.launcher_page = None;
                    if let Some(title) = &mut self.menu.title {
                        title.mode = horizon_client::title::Mode::Main;
                    }
                }
                LauncherEvent::PlayRow(id) => {
                    self.menu.launcher_page = None;
                    let saves = self
                        .menu
                        .title
                        .as_ref()
                        .map(|t| t.saves.clone())
                        .unwrap_or_default();
                    // The list this id came from is the cached one; a miss means the
                    // cache was cleared under the tap, so re-read rather than drop it.
                    let worlds = self
                        .menu
                        .launcher_worlds
                        .clone()
                        .unwrap_or_else(|| horizon_client::title::list_worlds(&saves));
                    if let Some(world) = worlds.into_iter().nth(id as usize) {
                        let name = self
                            .options
                            .as_ref()
                            .map_or_else(|| "Player".into(), |o| o.player_name());
                        self.menu.queue_entry(Action::Play { folder: world.folder }, name);
                    }
                }
                LauncherEvent::OpenSettings => {
                    // The launcher has its own Settings page now, so this stays on the
                    // glass instead of handing over to the game's options screen.
                    self.menu.launcher_page = Some(LauncherPage::Settings);
                }
                // The Skins page stays in the launcher. Its button used to hand over to
                // the game's own skin screen, which is a different page in a different
                // program; what the player asked for is a file picker, and the copy
                // lands in import/skins where the page already lists it.
                LauncherEvent::OpenSkins => {
                    if horizon_client::clipboard::pick_skin() {
                        self.menu.launcher_notice =
                            Some("Pick a 64x64 PNG. It appears in the library when you come back."
                                .to_string());
                    } else {
                        self.menu.launcher_notice = Some(
                            "No file picker on this device: drop the PNG in import/skins."
                                .to_string(),
                        );
                    }
                }
                LauncherEvent::PickSkin(name) => {
                    let import = horizon_client::gfx::import_dir();
                    let dir = import.join("skins");
                    horizon_client::skin_choice::save(&dir, Some(&name));
                    // The next frame uploads the new PNG: the cache is the file that
                    // was last shown, and it has to be forgotten or the page would keep
                    // drawing the old one.
                    self.uploaded_skin = None;
                    self.menu.launcher_notice = Some(format!("Skin set to {name}"));
                }
                LauncherEvent::RefreshMods => {
                    self.mod_registry = None;
                    self.mod_registry_rx = None;
                    self.menu.launcher_notice = None;
                }
                LauncherEvent::InstallMod(id) => {
                    let import = horizon_client::gfx::import_dir();
                    // Cloned out before the install: `mod_registry` borrows `self`, and
                    // the entry has to outlive that borrow to reach the download.
                    let entry = self
                        .mod_registry()
                        .and_then(|registry| registry.entries.iter().find(|e| e.id == id).cloned());
                    // A miss means the list was refreshed between the tap and here, which
                    // is not an error worth dropping the rest of this frame's events for.
                    if let Some(entry) = entry {
                        let notice = match horizon_client::mod_registry::install(&import, &entry) {
                            // The game reads `import/mods/` when a world opens, so a mod
                            // installed from here is live on the next world, not this one.
                            Ok(path) => format!(
                                "Installed {}. It loads with the next world.",
                                path.file_name()
                                    .map(|name| name.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| id.clone())
                            ),
                            Err(error) => format!("Install failed: {error}"),
                        };
                        self.menu.launcher_notice = Some(notice);
                        // The list is stale now: it says "Install" where the file is
                        // already there. Dropping it re-fetches on the next frame, and the
                        // notice above it survives because only the registry is cleared.
                        self.mod_registry = None;
                        self.mod_registry_rx = None;
                    }
                }
                LauncherEvent::OpenModsSite => {
                    if let Err(error) = horizon_client::clipboard::open_url(MODS_SITE) {
                        log::info!("launcher: could not open {MODS_SITE}: {error}");
                    }
                }
                LauncherEvent::SetOption(key, value) => {
                    self.apply_launcher_option(&key, value);
                }
                LauncherEvent::ToggleOption(key, on) => {
                    self.apply_launcher_option(&key, if on { 1.0 } else { 0.0 });
                }
                LauncherEvent::NewInstance => {
                    // There is one pack per build and the engine's protocol is pinned to
                    // it, so a second installation would be a second copy of the same
                    // version with no way to play anything else. Say that rather than
                    // drawing a button that does nothing.
                    self.menu.launcher_notice = Some(
                        "One installation per build: the engine is compiled for one version."
                            .to_string(),
                    );
                }
                LauncherEvent::SelectRow(id) => {
                    // On Installations there is exactly one row and it is already the
                    // selected one; on Multiplayer a row is a world, which the Host
                    // button handles. Nothing to do, and nothing to say.
                    log::info!("launcher: row {id} selected");
                }
                LauncherEvent::RevealFiles => {
                    // The activity owns intents, so it is what shows the folder. The
                    // launcher's own page lists it either way, so a device with no file
                    // manager still tells the player where the files are.
                    let import = horizon_client::gfx::import_dir();
                    if !horizon_client::clipboard::reveal_files("import") {
                        self.menu.launcher_notice =
                            Some(format!("Files live in {}", import.display()));
                    }
                }
                LauncherEvent::HostWorld(id) => self.host_world(id),
                LauncherEvent::StopHosting => {
                    self.hosting = None;
                    self.menu.launcher_notice = Some("Stopped hosting.".to_string());
                }
                LauncherEvent::AddAccount => {
                    // A new offline name is a rename away: the account card is the
                    // field, and this clears it so the next name is a new account.
                    self.menu.launcher_draft.clear();
                    self.menu.launcher_notice =
                        Some("Type a name for the new offline account.".to_string());
                }
                LauncherEvent::SwitchAccount(id) => self.switch_account(id),
                LauncherEvent::RemoveAccount(id) => self.remove_account(id),
                LauncherEvent::SignInMicrosoft => self.sign_in_microsoft(),
                LauncherEvent::SetControls(preset) => self.set_controls_preset(&preset),
                // Only reachable on the bring-up card, which has its own handler.
                LauncherEvent::Install | LauncherEvent::Restart => {}
                // Settings travel as one archive in the folder a file manager can
                // reach: the export writes it there, the import takes the newest one.
                LauncherEvent::ImportFiles(kind) => {
                    if !horizon_client::clipboard::pick_file(&kind) {
                        self.menu.launcher_notice = Some(
                            "No file picker on this device: drop the file in import/.".to_string(),
                        );
                    }
                }
                LauncherEvent::ExportFiles(kind) => {
                    if !horizon_client::clipboard::export_files(&kind) {
                        let import = horizon_client::gfx::import_dir().join(&kind);
                        self.menu.launcher_notice = Some(format!("Files live in {}", import.display()));
                    }
                }
                LauncherEvent::ExportSettings => {
                    let import = horizon_client::gfx::import_dir();
                    let home = std::env::current_dir().unwrap_or_default();
                    self.menu.launcher_notice =
                        Some(match horizon_client::settings_archive::export(&home, &import) {
                            // A phone cannot hand the player a path, so the archive is
                            // also written where the system file manager shows it.
                            Ok(path) => {
                                horizon_client::clipboard::export_files("settings");
                                format!("Saved to {}", path.display())
                            }
                            Err(error) => format!("Export failed: {error}"),
                        });
                }
                LauncherEvent::ImportSettings => {
                    let import = horizon_client::gfx::import_dir();
                    let home = std::env::current_dir().unwrap_or_default();
                    self.menu.launcher_notice =
                        Some(match horizon_client::settings_archive::newest(&import) {
                            None => "No archive in import/ to import".to_string(),
                            Some(archive) => {
                                match horizon_client::settings_archive::import(
                                    &archive, &home, &import,
                                ) {
                                    // The running game read its options at start, so
                                    // the honest instruction is to restart: applying
                                    // half of them now would show a mix of old and new.
                                    Ok(count) => format!("Imported {count} files. Restart to apply."),
                                    Err(error) => format!("Import failed: {error}"),
                                }
                            }
                        });
                }
                LauncherEvent::RenameAccount => self.menu.launcher_draft.clear(),
                LauncherEvent::SetAccount(name) => {
                    self.apply_account(&name);
                    self.menu.launcher_draft.clear();
                }
                LauncherEvent::Quit => event_loop.exit(),
            }
        }
    }

    /// Keep a player name the launcher was given. The file beside options.txt is what
    /// the next start reads, and an offline UUID is hashed from the name, so a world
    /// goes on knowing who it saved the inventory for.
    #[cfg(target_os = "android")]
    fn apply_account(&mut self, name: &str) {
        let name: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(16)
            .collect();
        if !horizon_client::offline_name::is_valid(&name) {
            log::info!("launcher: refused the name {name:?}");
            return;
        }
        if let Some(options) = &mut self.options {
            options.player_name = Some(name.clone());
        }
        let path = Game::options_path().with_file_name(horizon_client::offline_name::FILE);
        if let Err(error) = std::fs::write(&path, format!("name:{name}\n")) {
            log::warn!("Horizon: could not keep the player name: {error}");
        }
        self.save_menu_settings();
    }

    /// Switch to an account by its place in the list the page drew.
    #[cfg(target_os = "android")]
    fn switch_account(&mut self, id: u32) {
        let Some(account) = self.launcher_accounts().into_iter().nth(id as usize) else {
            return;
        };
        self.apply_account(&account.name);
        let import = horizon_client::gfx::import_dir();
        let mut public: Vec<(String, String)> = self
            .launcher_accounts()
            .into_iter()
            .map(|a| (if a.kind == "Microsoft" { "msa" } else { "offline" }.to_string(), a.name))
            .collect();
        public.retain(|(_, name)| name != &account.name);
        public.insert(
            0,
            (if account.kind == "Microsoft" { "msa" } else { "offline" }.to_string(), account.name.clone()),
        );
        let pairs: Vec<(&str, &str)> = public
            .iter()
            .map(|(kind, name)| (kind.as_str(), name.as_str()))
            .collect();
        horizon_client::msa::write_public(&import, &pairs);
        self.menu.launcher_notice = Some(format!("Playing as {}", account.name));
    }

    /// Forget an account. The Microsoft tokens go with it; an offline name is only
    /// ever a line in the list.
    #[cfg(target_os = "android")]
    fn remove_account(&mut self, id: u32) {
        let Some(account) = self.launcher_accounts().into_iter().nth(id as usize) else {
            return;
        };
        if account.kind == "Microsoft" {
            if let Some(stored) = horizon_client::msa::stored()
                .into_iter()
                .find(|a| a.name == account.name)
            {
                horizon_client::msa::forget(&stored.uuid);
            }
        }
        let import = horizon_client::gfx::import_dir();
        let remaining: Vec<(String, String)> = self
            .launcher_accounts()
            .into_iter()
            .filter(|a| a.name != account.name)
            .map(|a| (if a.kind == "Microsoft" { "msa" } else { "offline" }.to_string(), a.name))
            .collect();
        let pairs: Vec<(&str, &str)> = remaining
            .iter()
            .map(|(kind, name)| (kind.as_str(), name.as_str()))
            .collect();
        horizon_client::msa::write_public(&import, &pairs);
        self.menu.launcher_notice = Some(format!("Removed {}", account.name));
    }

    /// Start a Microsoft sign-in on a worker, and show the code to type.
    ///
    /// The flow is the device-code one: the phone asks Microsoft for a code, the
    /// player types it in any browser, and the worker polls until it is approved.
    /// Nothing here blocks a frame.
    #[cfg(target_os = "android")]
    fn sign_in_microsoft(&mut self) {
        let Some(client_id) = horizon_client::msa::client_id() else {
            self.menu.launcher_notice = Some(
                "Microsoft sign-in is not configured on this build: set HORIZON_MSA_CLIENT_ID."
                    .to_string(),
            );
            return;
        };
        let code = match horizon_client::msa::device_code(&client_id) {
            Ok(code) => code,
            Err(error) => {
                self.menu.launcher_notice = Some(format!("Sign-in failed: {error}"));
                return;
            }
        };
        self.menu.launcher_notice = Some(format!(
            "Go to {} and type {}",
            code.verification_uri, code.user_code
        ));
        // The browser is where the code is typed, so open it: a phone has no way to
        // copy a code out of a game surface.
        let _ = horizon_client::clipboard::open_url(&code.verification_uri);
        let (tx, rx) = std::sync::mpsc::channel();
        self.sign_in = Some(SignInState::Waiting(code.user_code.clone()));
        self.sign_in_rx = Some(rx);
        std::thread::Builder::new()
            .name("horizon-msa".into())
            .spawn(move || {
                let result = horizon_client::msa::wait_for_sign_in(&client_id, &code, &|| false)
                    .and_then(|tokens| horizon_client::msa::finish_sign_in(&tokens));
                if let Ok(account) = &result {
                    if let Err(error) = horizon_client::msa::store(account) {
                        log::warn!("launcher: could not keep the account's tokens: {error}");
                    }
                }
                let _ = tx.send(result.map(|account| account.name));
            })
            .ok();
    }

    /// Take a sign-in's result, once its worker has one.
    #[cfg(target_os = "android")]
    fn poll_sign_in(&mut self) {
        let Some(rx) = &self.sign_in_rx else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("The sign-in worker stopped.".to_string())
            }
        };
        self.sign_in_rx = None;
        match result {
            Ok(name) => {
                self.sign_in = Some(SignInState::Done(name.clone()));
                let import = horizon_client::gfx::import_dir();
                let mut public: Vec<(String, String)> = horizon_client::msa::read_public(&import);
                public.retain(|(_, existing)| existing != &name);
                public.insert(0, ("msa".to_string(), name.clone()));
                let pairs: Vec<(&str, &str)> = public
                    .iter()
                    .map(|(kind, name)| (kind.as_str(), name.as_str()))
                    .collect();
                horizon_client::msa::write_public(&import, &pairs);
                self.apply_account(&name);
                self.menu.launcher_notice = Some(format!("Signed in as {name}"));
            }
            Err(error) => {
                self.sign_in = Some(SignInState::Failed(error.clone()));
                self.menu.launcher_notice = Some(error);
            }
        }
    }

    /// Host a world from the list the Multiplayer page drew.
    ///
    /// A phone has no separate server process the launcher may start, so this is the
    /// engine's own integrated server opened to the network: the same code path as
    /// vanilla's "Open to LAN", reached from the launcher instead of from the pause
    /// menu. The address it prints is the one friends type.
    #[cfg(target_os = "android")]
    fn host_world(&mut self, id: u32) {
        let saves = self
            .menu
            .title
            .as_ref()
            .map(|t| t.saves.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default().join("saves"));
        let worlds = self
            .menu
            .launcher_worlds
            .clone()
            .unwrap_or_else(|| horizon_client::title::list_worlds(&saves));
        let Some(world) = worlds.into_iter().nth(id as usize) else {
            self.menu.launcher_notice = Some("That world is not on this device any more.".into());
            return;
        };
        let Some(address) = local_address() else {
            self.menu.launcher_notice =
                Some("No network address: join a Wi-Fi network first.".into());
            return;
        };
        // The engine opens the world on its server thread and publishes it; the
        // launcher records the address it will be reachable on.
        self.hosting = Some(format!("{address}:25565"));
        self.menu.launcher_notice = Some(format!(
            "Hosting {}. Friends join at {address}:25565",
            world.name
        ));
        self.menu.launcher_page = None;
        if let Some(title) = &mut self.menu.title {
            title.mode = horizon_client::title::Mode::Main;
        }
        self.menu.queue_entry(Action::Play { folder: world.folder }, "Host".into());
    }

    /// Switch the built-in touch layout.
    ///
    /// `controls.json` carries the buttons; the preset is a key in it, so the choice
    /// survives a restart and the engine reads the same file it always did. A layout
    /// the player has arranged themselves keeps its buttons and only changes the key.
    #[cfg(target_os = "android")]
    fn set_controls_preset(&mut self, preset: &str) {
        let path = horizon_client::gfx::controls_path();
        let mut layout = if preset == "bedrock" {
            bedrock_layout()
        } else {
            java_layout()
        };
        layout.preset = preset.to_string();
        if let Err(error) = layout.save(&path) {
            log::warn!("launcher: could not save the touch layout: {error}");
            self.menu.launcher_notice = Some(format!("Could not save the layout: {error}"));
            return;
        }
        // The HUD reads the layout once at start, so a preset saved here would only
        // appear after a restart. Re-reading it is what makes the button do what it
        // says.
        horizon_overlay::with(|o| o.reload_layout());
        self.menu.launcher_notice = Some(format!(
            "Touch controls: {}",
            if preset == "bedrock" { "Bedrock PE" } else { "Java Classic" }
        ));
    }

    /// The accounts the launcher offers: the signed-in one when there is one, plus
    /// every offline name this device has used.
    ///
    /// The desktop launcher keeps a whole `accounts.json` and a keychain. A phone has
    /// neither a keychain the launcher may reach nor a launcher process of its own, so
    /// the list is what `import/accounts.txt` holds — one `kind:name` line per account,
    /// the selected one marked — and the offline name beside `options.txt` is always
    /// one of them.
    #[cfg(target_os = "android")]
    fn launcher_accounts(&self) -> Vec<LauncherAccount> {
        let current = self
            .options
            .as_ref()
            .map_or_else(|| "Player".into(), |o| o.player_name());
        let mut accounts: Vec<LauncherAccount> = Vec::new();
        // The signed-in account, when the session file was read at start.
        if let Some(session) = horizon_client::auth::current() {
            accounts.push(LauncherAccount {
                name: session.name.clone(),
                kind: "Microsoft".into(),
                selected: session.name == current,
            });
        }
        for (kind, name) in horizon_client::msa::read_public(&horizon_client::gfx::import_dir()) {
            if accounts.iter().any(|a| a.name == name) {
                continue;
            }
            accounts.push(LauncherAccount {
                name: name.clone(),
                kind,
                selected: name == current,
            });
        }
        if !accounts.iter().any(|a| a.name == current) {
            accounts.insert(
                0,
                LauncherAccount {
                    name: current.clone(),
                    kind: "Offline".into(),
                    selected: true,
                },
            );
        }
        if !accounts.iter().any(|a| a.selected) {
            if let Some(first) = accounts.first_mut() {
                first.selected = true;
            }
        }
        accounts
    }

    /// The built-in touch layout in force, as the Profile page names it.
    #[cfg(target_os = "android")]
    fn launcher_controls_preset(&self) -> String {
        let path = horizon_client::gfx::controls_path();
        let preset = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.get("preset")?.as_str().map(str::to_owned));
        preset.unwrap_or_else(|| "java".to_string())
    }

    /// The address friends join on while this device hosts, if it does.
    #[cfg(target_os = "android")]
    fn hosting_address(&self) -> Option<String> {
        self.hosting.clone()
    }

    /// The mod registry, fetched at most once per visit to the Mods page.
    ///
    /// It is a network fetch, so it is kept between frames rather than repeated: the
    /// page redraws at the phone's refresh rate, and asking GitHub sixty times a second
    /// would be both slow and rude. Refresh clears it.
    #[cfg(target_os = "android")]
    fn mod_registry(&mut self) -> Option<&horizon_client::mod_registry::Registry> {
        // Poll a fetch already in flight. This is the whole reason the page does not
        // freeze: the network round trip is on a worker, and a frame only ever checks
        // whether the answer has arrived.
        if let Some(rx) = &self.mod_registry_rx {
            match rx.try_recv() {
                Ok(registry) => {
                    self.mod_registry = Some(registry);
                    self.mod_registry_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.mod_registry = Some(horizon_client::mod_registry::Registry {
                        status: "The registry worker stopped".into(),
                        failed: true,
                        ..Default::default()
                    });
                    self.mod_registry_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.mod_registry.is_none() && self.mod_registry_rx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            // A failure to spawn is not worth a dead page: the fetch then happens
            // inline, which is the old behaviour and still works.
            match std::thread::Builder::new()
                .name("horizon-mod-registry".into())
                .spawn(move || {
                    let _ = tx.send(horizon_client::mod_registry::fetch());
                }) {
                Ok(_) => self.mod_registry_rx = Some(rx),
                Err(error) => {
                    log::warn!("launcher: no worker for the mod registry ({error}); fetching inline");
                    self.mod_registry = Some(horizon_client::mod_registry::fetch());
                }
            }
        }
        self.mod_registry.as_ref()
    }

    /// The settings the launcher's own page offers, read from the live options so a
    /// slider shows the value the game is actually using rather than a copy that can
    /// drift. Keys are `options.txt` names; see `apply_launcher_option`.
    #[cfg(target_os = "android")]
    fn launcher_settings(&self) -> Vec<horizon_overlay::LauncherSetting> {
        use horizon_overlay::LauncherSetting;
        let f = &self.settings.file;
        let slider = |key: &str, label: &str, min: f32, max: f32, value: f32| LauncherSetting {
            key: key.to_string(),
            label: label.to_string(),
            value,
            min,
            max,
            toggle: false,
            on: false,
        };
        let toggle = |key: &str, label: &str, on: bool| LauncherSetting {
            key: key.to_string(),
            label: label.to_string(),
            value: if on { 1.0 } else { 0.0 },
            min: 0.0,
            max: 1.0,
            toggle: true,
            on,
        };
        vec![
            // Graphics.
            slider(
                "renderDistance",
                "Render distance (chunks)",
                2.0,
                32.0,
                f.int("renderDistance") as f32,
            ),
            slider(
                "simulationDistance",
                "Simulation distance",
                5.0,
                32.0,
                f.int("simulationDistance") as f32,
            ),
            slider(
                "guiScale",
                "GUI scale (0 = automatic)",
                0.0,
                4.0,
                f.int("guiScale") as f32,
            ),
            slider("fov", "Field of view", 30.0, 110.0, f.int("fov") as f32),
            slider("maxFps", "Frame limit", 10.0, 260.0, f.int("maxFps") as f32),
            slider(
                "gamma",
                "Brightness",
                0.0,
                1.0,
                f.double("gamma") as f32,
            ),
            slider(
                "entityDistanceScaling",
                "Entity distance",
                0.5,
                5.0,
                f.double("entityDistanceScaling") as f32,
            ),
            toggle("enableVsync", "V-Sync", f.bool("enableVsync")),
            toggle("bobView", "View bobbing", f.bool("bobView")),
            toggle("ao", "Smooth lighting", f.bool("ao")),
            toggle("vignette", "Vignette", f.bool("vignette")),
            // Controls.
            slider(
                "mouseSensitivity",
                "Look sensitivity",
                0.0,
                1.0,
                f.double("mouseSensitivity") as f32,
            ),
            toggle("autoJump", "Auto-jump", f.bool("autoJump")),
            toggle("toggleCrouch", "Toggle crouch", f.bool("toggleCrouch")),
            toggle("toggleSprint", "Toggle sprint", f.bool("toggleSprint")),
            toggle("invertYMouse", "Invert mouse Y", f.bool("invertYMouse")),
            // Sound.
            slider(
                "soundCategory_master",
                "Master volume",
                0.0,
                1.0,
                f.double("soundCategory_master") as f32,
            ),
            slider(
                "soundCategory_music",
                "Music",
                0.0,
                1.0,
                f.double("soundCategory_music") as f32,
            ),
            slider(
                "soundCategory_blocks",
                "Blocks",
                0.0,
                1.0,
                f.double("soundCategory_blocks") as f32,
            ),
            slider(
                "soundCategory_hostile",
                "Hostile mobs",
                0.0,
                1.0,
                f.double("soundCategory_hostile") as f32,
            ),
            // Interface.
            slider(
                "chatOpacity",
                "Chat opacity",
                0.0,
                1.0,
                f.double("chatOpacity") as f32,
            ),
            slider(
                "chatScale",
                "Chat scale",
                0.0,
                1.0,
                f.double("chatScale") as f32,
            ),
            slider(
                "menuBackgroundBlurriness",
                "Menu blur",
                0.0,
                10.0,
                f.int("menuBackgroundBlurriness") as f32,
            ),
            toggle("showSubtitles", "Subtitles", f.bool("showSubtitles")),
            toggle("autoSuggestions", "Command suggestions", f.bool("autoSuggestions")),
            toggle("chatLinks", "Clickable chat links", f.bool("chatLinks")),
        ]
    }

    /// Write one setting the launcher's own Settings page changed.
    ///
    /// `options.txt` is the file the game reads at start, so writing it is the whole
    /// job: the value is live for the next world, and the running menu reads it back
    /// through the same parse it uses for every other option. The keys are the ones
    /// `options_file.rs` names, so a slider here and the game's own options screen
    /// cannot disagree about what "render distance" is called.
    #[cfg(target_os = "android")]
    fn apply_launcher_option(&mut self, key: &str, value: f32) {
        let path = Game::options_path();
        let mut text = std::fs::read_to_string(&path).unwrap_or_default();
        // A float is written the way Java writes one: no trailing `.0` for a whole
        // number, because that is what the parser on both sides expects.
        let rendered = if (value - value.round()).abs() < 0.001 {
            format!("{}", value.round() as i64)
        } else {
            format!("{value:.3}")
        };
        let mut replaced = false;
        let mut lines: Vec<String> = Vec::new();
        for line in text.lines() {
            match line.split_once(':') {
                Some((name, _)) if name == key => {
                    lines.push(format!("{key}:{rendered}"));
                    replaced = true;
                }
                _ => lines.push(line.to_string()),
            }
        }
        if !replaced {
            lines.push(format!("{key}:{rendered}"));
        }
        text = lines.join("\n");
        text.push('\n');
        if let Err(error) = std::fs::write(&path, &text) {
            log::warn!("launcher: could not write {key}: {error}");
            self.menu.launcher_notice = Some(format!("Could not save {key}"));
            return;
        }
        // The live copy has to follow, or the next save from the game's own options
        // screen would write its stale value back over this one.
        let file = &mut self.settings.file;
        let parsed = match file.get(key) {
            Some(horizon_client::options_file::V::Bool(_)) => {
                Some(horizon_client::options_file::V::Bool(value >= 0.5))
            }
            Some(horizon_client::options_file::V::Int(_)) => {
                Some(horizon_client::options_file::V::Int(value.round() as i32))
            }
            Some(horizon_client::options_file::V::Double(_)) => {
                Some(horizon_client::options_file::V::Double(f64::from(value)))
            }
            _ => None,
        };
        if let Some(parsed) = parsed {
            self.settings.file.set(key, parsed);
        }
        // The live `Settings` is what the menu draws with, so it has to follow the
        // file: otherwise a slider moved in the launcher would not show until the game
        // restarted, which is a control that looks broken.
        self.reload_settings();
        // And what the window owns: gamma, present mode, GUI scale and the font atlas
        // are not in `Settings`, they are in the renderer and the atlas.
        self.apply_menu_settings();
        self.menu.launcher_notice = Some(format!("{key} set to {rendered}"));
    }

    /// Re-read `options.txt` into the live settings.
    ///
    /// The launcher writes the file directly, which is the whole point — the game reads
    /// the same file — but the copy already in memory is what this menu draws and
    /// plays with. Reading it back after a write is what makes a slider take effect
    /// without a restart.
    #[cfg(target_os = "android")]
    fn reload_settings(&mut self) {
        let path = Game::options_path();
        let mut fresh = horizon_client::settings::Settings::load(&path);
        // The launcher's own sidecars are not in options.txt, so they survive the read.
        fresh.horizon = self.settings.horizon.clone();
        fresh.controller = self.settings.controller.clone();
        fresh.extras = self.settings.extras.clone();
        fresh.mods = self.settings.mods.clone();
        fresh.skins = self.settings.skins.clone();
        fresh.skin = self.settings.skin.clone();
        self.settings = fresh;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// A key press over the launcher's name field.
    #[cfg(target_os = "android")]
    fn launcher_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };
        match code {
            KeyCode::Backspace | KeyCode::Delete => {
                self.menu.launcher_draft.pop();
            }
            KeyCode::Enter => {
                let name = std::mem::take(&mut self.menu.launcher_draft);
                self.apply_account(&name);
                let _ = horizon_overlay::with(|o| o.end_launcher_edit());
            }
            KeyCode::Escape => {
                self.menu.launcher_draft.clear();
                let _ = horizon_overlay::with(|o| o.end_launcher_edit());
            }
            _ => {
                if let Some(ch) = typed_char(code, event.text.as_deref(), self.menu.modifiers.shift_key())
                    && self.menu.launcher_draft.chars().count() < 16
                {
                    self.menu.launcher_draft.push(ch);
                }
            }
        }
    }

    /// Whether whatever is on the screen takes letters — the same question the world
    /// asks for the phone's keyboard. The title's list has its fields, and the options
    /// screen a search box, and neither has a hardware key to fall back on.
    pub(super) fn menu_typing(&mut self) -> bool {
        #[cfg(target_os = "android")]
        let launcher = horizon_overlay::with(|o| o.launcher_editing()).unwrap_or(false);
        #[cfg(target_os = "android")]
        if launcher {
            return true;
        }
        self.menu.typing()
    }

    /// The phone's keyboard over the loading screen's menu: the title's focused
    /// field, or the options' search box. The words are given to the same handlers
    /// the menu's own keys go to, so the screen never learns where they came from.
    #[cfg(target_os = "android")]
    pub(super) fn menu_ime(&mut self) {
        let Some(input) = horizon_client::soft_keyboard::poll() else {
            return;
        };
        if self.gui.is_none() || self.menu.pending.is_some() || self.menu.win.is_some() {
            return;
        }
        // The launcher's name field is a string the card draws, not an imgui widget:
        // the phone's keyboard commits words to the activity, and imgui would want a
        // key at a time. Everything else about it is the same edit the title's fields
        // take.
        if horizon_overlay::with(|o| o.launcher_editing()).unwrap_or(false) {
            for _ in 0..input.backspaces {
                self.menu.launcher_draft.pop();
            }
            for ch in input.text.chars() {
                if (ch.is_ascii_alphanumeric() || ch == '_')
                    && self.menu.launcher_draft.chars().count() < 16
                {
                    self.menu.launcher_draft.push(ch);
                }
            }
            if input.enter {
                let name = self.menu.launcher_draft.clone();
                self.apply_account(&name);
                self.menu.launcher_draft.clear();
                let _ = horizon_overlay::with(|o| o.end_launcher_edit());
            }
            return;
        }
        if self.menu.options_open {
            for _ in 0..input.backspaces {
                let Some(gui) = &self.gui else { return };
                let events =
                    self.menu
                        .options
                        .key(&mut self.settings, &gui.atlas, KeyCode::Backspace, None);
                self.menu_option_events(events);
            }
            if !input.text.is_empty() {
                let Some(gui) = &self.gui else { return };
                self.menu.options.type_text(&gui.atlas, &input.text);
            }
            if input.enter {
                let Some(gui) = &self.gui else { return };
                let events = self
                    .menu
                    .options
                    .key(&mut self.settings, &gui.atlas, KeyCode::Enter, None);
                self.menu_option_events(events);
            }
            return;
        }
        let Some(title) = self.menu.title.as_mut() else {
            return;
        };
        for _ in 0..input.backspaces {
            title.backspace();
        }
        if !input.text.is_empty() {
            title.type_text(&input.text);
        }
        if input.enter {
            // Only a form's own submit comes back from here, and `menu_ime` has no
            // event loop of its own: queuing the entry is what a click does too.
            let action = title.confirm_edit();
            if !matches!(action, Action::None) {
                let name = self
                    .options
                    .as_ref()
                    .map_or_else(|| "Player".into(), |o| o.player_name());
                self.menu.queue_entry(action, name);
            }
        }
        self.save_menu_address();
    }

    /// Select All in the field the keyboard is about to write into: the phone's
    /// field is empty, and the game's is the whole value the first word replaces.
    #[cfg(target_os = "android")]
    pub(super) fn menu_ime_select_all(&mut self) {
        // The launcher's field opens empty on purpose: there is nothing to replace, so
        // the first word is the beginning of the name rather than all of it.
        if horizon_overlay::with(|o| o.launcher_editing()).unwrap_or(false) {
            return;
        }
        if self.menu.options_open {
            let Some(gui) = &self.gui else { return };
            if self.menu.options.typing() {
                self.menu.options.shortcut(
                    &gui.atlas,
                    clipboard::EditShortcut::SelectAll,
                    &mut self.menu.clipboard,
                );
            }
            return;
        }
        self.menu.ime_select_all();
    }

    pub(super) fn menu_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        event: WindowEvent,
    ) -> Result<()> {
        if let WindowEvent::KeyboardInput { event, .. } = &event
            && let PhysicalKey::Code(key) = event.physical_key
        {
            if event.state == ElementState::Pressed {
                self.menu.physical_keys.insert(key);
            } else {
                self.menu.physical_keys.remove(&key);
            }
        }
        match event {
            WindowEvent::CloseRequested => {
                self.save_menu_settings();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
                self.apply_menu_settings();
            }
            WindowEvent::RedrawRequested => {
                #[cfg(target_os = "android")]
                self.sync_launcher(event_loop);
                self.draw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.menu.modifiers = modifiers.state(),
            WindowEvent::Focused(false) => {
                self.menu.modifiers = ModifiersState::empty();
                self.menu.physical_keys.clear();
                self.menu.dragging = false;
                if let Some(title) = &mut self.menu.title {
                    title.release();
                    title.release_field();
                }
                let events = self.menu.options.release();
                self.menu_option_events(events);
            }
            // The launcher's name field takes keys before anything else does. The
            // phone's keyboard commits whole words to the IME hook, but a hardware
            // keyboard sends no text at all on Android, and a name is exactly the
            // thing a player with a keyboard in their lap wants to type.
            #[cfg(target_os = "android")]
            WindowEvent::KeyboardInput { event, .. }
                if horizon_overlay::with(|o| o.launcher_editing()).unwrap_or(false) =>
            {
                self.launcher_key(&event);
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && self.menu.pending.is_some() =>
            {
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape) {
                    self.menu.pending = None;
                    self.menu.perf.cancel_world_opening();
                }
            }
            _ if self.gui.is_none() || self.menu.pending.is_some() => {}
            WindowEvent::KeyboardInput { event, .. } if self.menu.win.is_some() => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let pressed = event.state == ElementState::Pressed;
                    if code == KeyCode::Escape && pressed {
                        self.menu.win = None;
                    } else if let Some(key) = horizon_client::win_screen::Key::from_code(code)
                        && let Some((win, _)) = &mut self.menu.win
                    {
                        win.key(key, pressed);
                    }
                }
            }
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
                if self.menu.win.is_some() => {}
            WindowEvent::CursorMoved { position, .. } => {
                self.menu.pointer = (position.x, position.y);
                self.update_menu_cursor();
                if self.menu.options_open && self.menu.dragging {
                    self.menu_options_click(true, false);
                } else if let Some(title) = &mut self.menu.title {
                    let size = self
                        .renderer
                        .as_ref()
                        .map_or([1280, 720], |r| [r.config.width, r.config.height]);
                    let b = HudBatch::new(size);
                    title.drag(self.menu.cursor, (b.width, b.height));
                    title.drag_field(&self.gui.as_ref().unwrap().atlas, self.menu.cursor.0);
                }
            }
            WindowEvent::MouseInput { state, button, .. } if self.menu.options_open => {
                if state == ElementState::Pressed && self.menu.options.selected_key.is_some() {
                    let events = self.menu.options.bind_mouse(&mut self.settings, button);
                    self.menu_option_events(events);
                } else if matches!(button, MouseButton::Left | MouseButton::Right) {
                    if state == ElementState::Pressed {
                        self.menu.dragging = button == MouseButton::Left;
                        self.menu_options_click(false, button == MouseButton::Right);
                    } else if button == MouseButton::Left {
                        self.menu.dragging = false;
                        let events = self.menu.options.release();
                        self.menu_option_events(events);
                    }
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let batch = HudBatch::new(self.menu_size());
                if let (Some(title), Some(gui)) = (&mut self.menu.title, &self.gui) {
                    if state == ElementState::Released {
                        title.release();
                        title.release_field();
                    } else {
                        let (action, _) = title.click(
                            self.menu.cursor.0,
                            self.menu.cursor.1,
                            (batch.width, batch.height),
                        );
                        title.press_field(
                            &gui.atlas,
                            self.menu.cursor,
                            (batch.width, batch.height),
                            self.menu.modifiers.shift_key(),
                        );
                        self.menu_action(event_loop, action);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let raw = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 30.0) as f32,
                };
                let amount = if self.settings.discrete_scroll {
                    horizon_client::mouse::java_signum(f64::from(raw)) as f32
                } else {
                    raw
                } * self.settings.mouse_wheel_sensitivity;
                let batch = HudBatch::new(self.menu_size());
                if self.menu.options_open {
                    self.menu.options.scroll_at(
                        &self.settings,
                        (batch.width, batch.height),
                        self.menu.cursor,
                        amount,
                    );
                } else if let Some(title) = &mut self.menu.title {
                    title.scroll_worlds(amount, self.menu.cursor, (batch.width, batch.height));
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if event.physical_key == PhysicalKey::Code(KeyCode::F11) {
                    self.settings.fullscreen = !self.settings.fullscreen;
                    self.apply_menu_settings();
                    self.save_menu_settings();
                } else if self.menu.options_open {
                    let shortcut = self.menu.shortcut(&event);
                    let atlas = &self.gui.as_ref().unwrap().atlas;
                    let handled = shortcut.is_some_and(|s| {
                        self.menu
                            .options
                            .shortcut(atlas, s, &mut self.menu.clipboard)
                    });
                    if !handled && let PhysicalKey::Code(key) = event.physical_key {
                        let events = self.menu.options.key(
                            &mut self.settings,
                            atlas,
                            key,
                            event.text.as_deref(),
                        );
                        self.menu_option_events(events);
                    }
                } else {
                    if let Some(action) = self.menu.title_key(&event) {
                        self.menu_action(event_loop, action);
                    }
                    self.save_menu_address();
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn draw_menu(&mut self, batch: &mut HudBatch) {
        // The launcher is drawn in imgui over the whole screen; the title's own quads
        // stand down while it is up, but the world-loading progress still draws.
        #[cfg(target_os = "android")]
        if self.menu.launcher_page.is_some() && self.menu.pending.is_none() {
            return;
        }
        let Some(gui) = &self.gui else { return };
        if self.menu.pending.is_some() {
            horizon_client::level_loading::draw_progress(
                batch,
                &gui.atlas,
                self.progress.line(),
                self.progress.fraction(),
            );
        } else if let Some((win, frame)) = &mut self.menu.win {
            let now = Instant::now();
            // 20 ticks a second: the startup menu runs no tick rate.
            let delta_ticks = now.duration_since(*frame).as_secs_f32() * 20.0;
            *frame = now;
            win.draw(batch, &gui.atlas, delta_ticks, Default::default());
            if win.finished(batch.height) {
                self.menu.win = None;
            }
        } else if self.menu.options_open {
            self.menu
                .options
                .draw(&self.settings, batch, &gui.atlas, self.menu.cursor);
        } else if let Some(title) = &mut self.menu.title {
            title.poll_backup();
            let _ = title.update_toast(self.settings.notification_display_time);
            title.draw(
                &gui.atlas,
                batch,
                self.menu.cursor,
                self.settings.file.bool("hideSplashTexts"),
            );
        }
    }

    pub(super) fn restart_menu_resources(&mut self) -> Result<()> {
        if !self.menu.reload_packs || self.loaded.is_none() {
            return Ok(());
        }
        if let Some(Ok(loaded)) = &mut self.loaded {
            if let Some(loader) = &loaded.rest_loader
                && !loader.ready()?
            {
                return Ok(());
            }
            if let Some(loader) = loaded.rest_loader.take()
                && let Err(error) = loader.finish()
            {
                log::warn!("Horizon: replaced resource generation failed: {error}");
            }
        }
        self.loaded = None;
        if let Some(handle) = self.handle.take() {
            handle.join().map_err(|_| "resource loader panicked")?;
        }
        self.progress = Progress::new();
        let options = self.options.as_ref().ok_or("loading already finished")?;
        let (receiver, handle) = spawn_loader(
            options,
            &self.settings,
            &self.vanilla_root,
            self.started,
            &self.progress,
        )?;
        self.receiver = receiver;
        self.handle = Some(handle);
        self.menu.reload_packs = false;
        Ok(())
    }
}

/// The character a key stands for in a player name. Where the platform gives winit a
/// string for the press — a phone's keyboard, an IME's dead key — that wins; a hardware
/// key on Android gives nothing, so the physical code is read for itself. Only what a
/// Java name may hold comes back.
#[cfg(target_os = "android")]
fn typed_char(code: KeyCode, text: Option<&str>, shift: bool) -> Option<char> {
    // Whatever the platform says the key produced is taken as given: it knows about
    // layout, dead keys and case, and the code below knows about none of them.
    if let Some(ch) = text.and_then(|t| t.chars().next()) {
        return (ch.is_ascii_alphanumeric() || ch == '_').then_some(ch);
    }
    let plain = match code {
        KeyCode::KeyA => 'a',
        KeyCode::KeyB => 'b',
        KeyCode::KeyC => 'c',
        KeyCode::KeyD => 'd',
        KeyCode::KeyE => 'e',
        KeyCode::KeyF => 'f',
        KeyCode::KeyG => 'g',
        KeyCode::KeyH => 'h',
        KeyCode::KeyI => 'i',
        KeyCode::KeyJ => 'j',
        KeyCode::KeyK => 'k',
        KeyCode::KeyL => 'l',
        KeyCode::KeyM => 'm',
        KeyCode::KeyN => 'n',
        KeyCode::KeyO => 'o',
        KeyCode::KeyP => 'p',
        KeyCode::KeyQ => 'q',
        KeyCode::KeyR => 'r',
        KeyCode::KeyS => 's',
        KeyCode::KeyT => 't',
        KeyCode::KeyU => 'u',
        KeyCode::KeyV => 'v',
        KeyCode::KeyW => 'w',
        KeyCode::KeyX => 'x',
        KeyCode::KeyY => 'y',
        KeyCode::KeyZ => 'z',
        KeyCode::Digit0 => '0',
        KeyCode::Digit1 => '1',
        KeyCode::Digit2 => '2',
        KeyCode::Digit3 => '3',
        KeyCode::Digit4 => '4',
        KeyCode::Digit5 => '5',
        KeyCode::Digit6 => '6',
        KeyCode::Digit7 => '7',
        KeyCode::Digit8 => '8',
        KeyCode::Digit9 => '9',
        KeyCode::Minus => '_',
        _ => return None,
    };
    let ch = if shift {
        plain.to_ascii_uppercase()
    } else {
        plain
    };
    (ch.is_ascii_alphanumeric() || ch == '_').then_some(ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gui(root: &std::path::Path) -> UiResources {
        UiResources {
            atlas: HudAtlas::fonts(root).unwrap(),
            panorama: Arc::new(horizon_client::gfx::panorama::Faces {
                size: 1,
                pixels: [96, 110, 111, 255].repeat(6),
            }),
        }
    }

    fn loading(receiver: Receiver<LoaderMsg>) -> Loading {
        Loading {
            window: None,
            renderer: None,
            gui: None,
            settings: Settings::default(),
            fullscreen: horizon_client::window::FullscreenState::new(false),
            options: None,
            vanilla_root: PathBuf::new(),
            pack_error: None,
            started: Instant::now(),
            panorama_rotation: Default::default(),
            progress: Progress::new(),
            receiver,
            loaded: None,
            handle: None,
            shown: false,
            menu: StartupMenu::default(),
            error: None,
        }
    }

    #[test]
    fn menu_is_usable_while_the_core_worker_is_blocked() {
        let root = std::env::temp_dir().join("horizon-startup-menu-no-world-resources");
        let (sender, receiver) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let (published, published_rx) = std::sync::mpsc::channel();
        let worker_root = root.clone();
        let worker = std::thread::spawn(move || {
            sender
                .send(LoaderMsg::Ui(gui(&worker_root), worker_root))
                .unwrap();
            published.send(()).unwrap();
            wait.recv().unwrap();
            sender
                .send(LoaderMsg::Done(Err("core failure".into())))
                .unwrap();
        });
        published_rx.recv().unwrap();
        let mut loading = loading(receiver);
        loading.poll_menu_resources().unwrap();
        assert!(loading.gui.is_some());
        assert!(loading.loaded.is_none());
        assert!(!worker.is_finished());
        // Navigate and enter a world name without a registry, biomes, pack
        // tables or a Game. Replacing fallback artwork preserves this state.
        let title = loading.menu.title.as_mut().unwrap();
        title.mode = horizon_client::title::Mode::Create {
            name: String::new(),
            seed: "42".into(),
            creative: true,
            focus: 0,
            options: Default::default(),
        };
        title.type_text("My world");
        loading.set_ui(gui(&root), root);
        let action = loading.menu.title.as_ref().unwrap().create_action();
        loading.menu.queue_entry(action, "Player".into());
        assert!(
            matches!(&loading.menu.pending, Some(PendingWorldEntry::Local {
            name: Some(name), seed: Some(42), creative: true, create: Some(_), ..
        }) if name == "My world")
        );
        let mut batch = HudBatch::new([1280, 720]);
        loading.draw_menu(&mut batch);
        assert!(
            !batch.vertices.is_empty(),
            "queued entry draws loading progress"
        );
        release.send(()).unwrap();
        worker.join().unwrap();
        loading.poll_menu_resources().unwrap();
        assert!(matches!(loading.loaded, Some(Err(ref error)) if error == "core failure"));
    }

    #[test]
    fn queued_server_keeps_address_and_player_identity() {
        let mut menu = StartupMenu::default();
        menu.queue_entry(
            Action::Connect {
                address: "localhost:25565".into(),
            },
            "Guest".into(),
        );
        assert!(
            matches!(menu.pending.take(), Some(PendingWorldEntry::Server { address, name })
            if address == "localhost:25565" && name == "Guest")
        );
        assert!(menu.pending.take().is_none(), "an entry is consumed once");
    }

    #[test]
    fn disconnected_core_worker_is_reported_instead_of_waiting_forever() {
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(sender);
        let mut loading = loading(receiver);
        assert_eq!(
            loading.poll_menu_resources().unwrap_err().to_string(),
            "resource loader stopped before finishing"
        );
    }
}

/// The launcher's News page. Horizon has no feed to fetch and no server to ask, so
/// these are the things a player of this build actually needs to know, written where
/// they will read them. The first card is the one that is always true.
#[cfg(target_os = "android")]
/// This device's address on the network it is on, for the address friends type.
/// A phone has no hostname a stranger can resolve, so this is the IPv4 of the first
/// interface that has one and is not the loopback.
#[cfg(target_os = "android")]
fn local_address() -> Option<String> {
    use std::net::UdpSocket;
    // Connecting a UDP socket sends nothing; it only asks the routing table which
    // local address would reach the outside world, which is the one a friend on the
    // same network can reach too.
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let address = socket.local_addr().ok()?.ip();
    (!address.is_loopback() && !address.is_unspecified()).then(|| address.to_string())
}

/// The Java Classic touch layout: the cross on the left, the two mouse buttons and
/// jump on the right, the hotbar across the top. This is the layout Horizon shipped
/// before the choice existed.
#[cfg(target_os = "android")]
fn java_layout() -> horizon_controls::Layout {
    horizon_controls::Layout::default()
}

/// The Bedrock PE touch layout: the same actions, arranged the way the phone version
/// of the game puts them — the cross is a diamond rather than a cross, jump sits at
/// the bottom right beside the two mouse buttons, and the hotbar is a row of nine at
/// the bottom centre, where a thumb can sweep it.
///
/// The numbers are fractions of the glass, so they hold on any phone: no button's
/// middle falls inside another's, which is the one mistake a HUD layout cannot
/// recover from.
#[cfg(target_os = "android")]
fn bedrock_layout() -> horizon_controls::Layout {
    use horizon_controls::{Action, Layout};
    let mut layout = Layout::default();
    layout.controls.clear();
    let mut put = |action: Action, x: f32, y: f32, size: f32| {
        layout.controls.push(horizon_controls::Control {
            action,
            center: [x, y],
            size,
            opacity: None,
        });
    };
    // Movement: a diamond, not a cross.
    put(Action::Forward, 0.105, 0.635, 0.10);
    put(Action::Back, 0.105, 0.895, 0.10);
    put(Action::Left, 0.038, 0.765, 0.10);
    put(Action::Right, 0.172, 0.765, 0.10);
    put(Action::Sneak, 0.245, 0.885, 0.09);
    put(Action::Sprint, 0.245, 0.755, 0.085);
    // Right thumb: jump above the two mouse buttons, which is where a phone player's
    // thumb rests.
    put(Action::Jump, 0.905, 0.66, 0.115);
    put(Action::Mine, 0.855, 0.885, 0.145);
    put(Action::Use, 0.705, 0.885, 0.12);
    put(Action::Fly, 0.70, 0.66, 0.085);
    put(Action::Pick, 0.955, 0.44, 0.075);
    // The bar along the top: menu on the left, then the row of small keys.
    put(Action::Menu, 0.04, 0.06, 0.075);
    put(Action::Pause, 0.04, 0.17, 0.075);
    put(Action::Inventory, 0.55, 0.17, 0.08);
    put(Action::Drop, 0.63, 0.17, 0.08);
    put(Action::Chat, 0.71, 0.17, 0.08);
    put(Action::Command, 0.79, 0.17, 0.08);
    put(Action::TabList, 0.87, 0.17, 0.08);
    put(Action::Debug, 0.95, 0.17, 0.08);
    put(Action::View, 0.955, 0.06, 0.075);
    put(Action::SwapOffhand, 0.16, 0.06, 0.05);
    put(Action::Screenshot, 0.22, 0.06, 0.05);
    // The hotbar along the bottom centre, the row the thumb sweeps.
    for slot in 1..=9u8 {
        put(
            Action::Hotbar(slot),
            0.36 + f32::from(slot - 1) * 0.038,
            0.945,
            0.052,
        );
    }
    layout
}

/// The PNG's pixel size, read from its IHDR without decoding the pixels: the Skins
/// page needs the dimensions for its UVs, and a 64x64 skin is not worth a full
/// decode on a frame.
#[cfg(target_os = "android")]
fn png_size(path: &std::path::Path) -> Option<[u32; 2]> {
    let data = std::fs::read(path).ok()?;
    // 8 bytes of signature, then the IHDR chunk: 4 length, 4 type, 4 width, 4 height.
    if data.len() < 24 || &data[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(data[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(data[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some([width, height])
}

/// The folders the game reads, as the Files page lists them: what is inside
/// `import/`, and how much of it there is.
#[cfg(target_os = "android")]
fn files_rows() -> Vec<horizon_overlay::LauncherRow> {
    let import = horizon_client::gfx::import_dir();
    let mut rows = Vec::new();
    let Ok(entries) = std::fs::read_dir(&import) else {
        return rows;
    };
    let mut dirs: Vec<(String, usize)> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let count = std::fs::read_dir(entry.path())
                .map(|inner| inner.flatten().count())
                .unwrap_or(0);
            (name, count)
        })
        .collect();
    dirs.sort_by(|a, b| a.0.cmp(&b.0));
    for (id, (name, count)) in dirs.into_iter().enumerate() {
        rows.push(horizon_overlay::LauncherRow {
            id: id as u32,
            title: name.clone(),
            subtitle: match count {
                0 => format!("{}/ · empty", import.display()),
                1 => format!("{}/ · 1 file", import.display()),
                n => format!("{}/ · {n} files", import.display()),
            },
            badge: String::new(),
            selected: false,
            enabled: false,
        });
    }
    rows
}

#[cfg(target_os = "android")]
fn news_stories() -> Vec<horizon_overlay::LauncherStory> {
    use horizon_overlay::LauncherStory;
    let story = |title: &str, tag: &str, body: &str| LauncherStory {
        title: title.to_string(),
        tag: tag.to_string(),
        body: body.to_string(),
    };
    vec![
        story(
            "Horizon runs natively on Android",
            "THIS BUILD",
            "No Java, no emulator and no compatibility layer: the engine is Rust compiled \
             to arm64, drawing through Vulkan. Minecraft's own files are read as data — the \
             client JAR supplies textures, models and world data, and the game logic is \
             Horizon's own.",
        ),
        story(
            "One version at a time",
            "HOW IT WORKS",
            "This build plays Minecraft Java 26.3. The engine's protocol, registry and block \
             tables are generated for one version, so the Versions list shows what exists and \
             says plainly that anything else is not supported. Porting another version is a \
             new build, not a download.",
        ),
        story(
            "Sound packs fill in on their own",
            "FIRST LAUNCH",
            "The client JAR carries no sounds. They are separate objects in Mojang's asset \
             index, and the launcher fetches them in the background while you are already in \
             the menu. A pass that is interrupted resumes where it stopped, so a slow \
             connection costs nothing.",
        ),
        story(
            "Skins come from a file",
            "SKINS",
            "There is no account sign-in on a phone, so a skin is a PNG you drop into \
             import/skins and pick on the Skins page. The choice is kept in horizon-skin.txt \
             beside options.txt and applies to your player model.",
        ),
        story(
            "Mods need a runtime this build does not carry",
            "MODS",
            "Android forbids the executable memory a JIT needs, so Horizon's WebAssembly mods \
             cannot run here. Modpacks ported to 26.3 do work as resource and data packs.",
        ),
    ]
}

/// The launcher's Patch Notes page: what changed in the build the player is holding.
/// Newest first, and written for someone who wants to know whether their problem was
/// fixed rather than what was refactored.
#[cfg(target_os = "android")]
#[cfg(target_os = "android")]
fn patch_stories() -> Vec<horizon_overlay::LauncherStory> {
    use horizon_overlay::LauncherStory;
    let story = |title: &str, tag: &str, body: &str| LauncherStory {
        title: title.to_string(),
        tag: tag.to_string(),
        body: body.to_string(),
    };
    vec![
        story(
            "0.1.2 — the launcher is a launcher",
            "LATEST",
            "The front page is the title panorama with Play over it, and a column of \
             destinations beside it. Skins, Settings, News and Patch Notes are pages in the \
             launcher now, so changing a skin or the render distance no longer means entering \
             the game. Settings write options.txt directly, so the launcher and the game's own \
             options screen cannot disagree.",
        ),
        story(
            "0.1.2 — every tap lands",
            "FIXED",
            "No control on the launcher responded: the release imgui needs to turn a press \
             into a click was being cleared on the frame after it arrived. Also fixed: the app \
             died on its first painted frame because two draw lists were open on the same \
             window at once.",
        ),
        story(
            "0.1.2 — sounds arrive in minutes, not hours",
            "FASTER",
            "The sound pack used to be fetched one small file at a time. It is a pool of \
             connections now, and files already on disk are counted before it starts, so a \
             resumed pass neither repeats work nor reports itself as starting from zero.",
        ),
        story(
            "0.1.2 — first launch no longer freezes",
            "FIXED",
            "The install used to run before the app could draw, which Android treats as an \
             ANR. The bring-up card owns it now and keeps drawing while it downloads.",
        ),
        story(
            "0.1.1 — an icon, and an opening",
            "RELEASED",
            "The APK has a real adaptive icon and an API 31 splash, so it no longer opens on a \
             black flash, and the launcher fades its interface in behind a wordmark.",
        ),
    ]
}
