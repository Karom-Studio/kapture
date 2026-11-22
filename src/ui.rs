use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use global_hotkey::hotkey::HotKey;
use global_hotkey::HotKeyState;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use gpui::{
    img, px, rgb, size, App, Bounds, Context, ImageSource, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, Render, RenderImage, SharedString, Window,
    WindowBounds, WindowOptions, div, prelude::*,
};
use image::RgbaImage;
use smallvec::smallvec;

use crate::copy;
use crate::dto::{ActionResult, Bootstrap, CaptureDto, Notice, RecordingStatus};
use crate::engine::Engine;
use crate::error::CommandError;
use crate::settings::{Corner, Hotkeys, Quality, Settings, VideoHeight, WebcamSize};

enum Page { Home, Library, Settings }
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode { Browse, Preparing, Region, Recording }
#[derive(Clone, Copy)]
enum HotAct { ShotScreen, ShotWindow, ShotRegion, RecordToggle, RecordPause }
enum Job {
    Done(Result<ActionResult, String>),
    Region(Result<(u32, u32, RgbaImage), String>),
    Status(Result<RecordingStatus, String>, &'static str),
    Windows { intent: String, rows: Vec<crate::dto::WindowDto> },
    Folder(Option<std::path::PathBuf>),
    Command(HotAct),
}

pub fn run() {
    let (tx, rx) = mpsc::channel();
    gpui::Application::new().run(move |cx: &mut App| {
        #[cfg(target_os = "linux")]
        {
            let _ = gtk::init();
        }
        install_tray(tx.clone());
        let bounds = Bounds::centered(None, size(px(1120.), px(740.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |_, cx| cx.new(|cx| Kapture::new(rx, tx.clone(), cx)),
        )
        .expect("fenêtre Kapture Desktop");
        cx.activate(true);
    });
}

struct Kapture {
    engine: Arc<Engine>,
    tx: Sender<Job>,
    rx: Receiver<Job>,
    boot: Bootstrap,
    page: Page,
    mode: Mode,
    notice: Option<Notice>,
    warnings: Vec<Notice>,
    preview: Option<Arc<RenderImage>>,
    preview_w: u32,
    preview_h: u32,
    drag: Option<(Point<Pixels>, Point<Pixels>)>,
    picker: Option<(String, Vec<crate::dto::WindowDto>)>,
    manager: Option<GlobalHotKeyManager>,
    registered: Vec<HotKey>,
    hot_ids: HashMap<u32, HotAct>,
    busy: bool,
    started: bool,
}

impl Kapture {
    fn new(rx: Receiver<Job>, tx: Sender<Job>, cx: &mut Context<Self>) -> Self {
        let engine = Arc::new(Engine::new());
        let boot = engine.bootstrap();
        let view = Self {
            engine,
            tx,
            rx,
            boot,
            page: Page::Home,
            mode: Mode::Browse,
            notice: None,
            warnings: Vec::new(),
            preview: None,
            preview_w: 0,
            preview_h: 0,
            drag: None,
            picker: None,
            manager: GlobalHotKeyManager::new().ok(),
            registered: Vec::new(),
            hot_ids: HashMap::new(),
            busy: false,
            started: false,
        };
        cx.spawn(async move |this, cx: &mut gpui::AsyncApp| {
            loop {
                cx.background_executor().timer(Duration::from_millis(140)).await;
                if this.update(cx, |view, cx| view.poll(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        view
    }

    fn poll(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            match event.id.0.as_str() {
                "shot-screen" => self.fire(HotAct::ShotScreen, cx),
                "shot-region" => self.fire(HotAct::ShotRegion, cx),
                "record" => self.fire(HotAct::RecordToggle, cx),
                "quit" => {
                    self.engine.finalize();
                    std::process::exit(0);
                }
                _ => {}
            }
        }
        if !self.started {
            self.started = true;
            self.rebind();
        }
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == HotKeyState::Pressed {
                if let Some(act) = self.hot_ids.get(&event.id).copied() {
                    self.fire(act, cx);
                }
            }
        }
        while let Ok(job) = self.rx.try_recv() {
            self.busy = false;
            match job {
                Job::Done(result) => self.apply(result, cx),
                Job::Region(result) => match result {
                    Ok((w, h, image)) => {
                        self.preview = Some(rgba_image(&image));
                        self.preview_w = w;
                        self.preview_h = h;
                        self.mode = Mode::Region;
                        self.drag = None;
                    }
                    Err(message) => self.fail_text(message),
                },
                Job::Status(result, code) => match result {
                    Ok(status) => {
                        self.boot.recording = status;
                        self.mode = Mode::Recording;
                        self.notice = Some(note(&self.boot.settings.locale, "info", code));
                    }
                    Err(message) => {
                        self.mode = Mode::Browse;
                        self.fail_text(message);
                    }
                },
                Job::Windows { intent, rows } => self.picker = Some((intent, rows)),
                Job::Command(act) => self.fire(act, cx),
                Job::Folder(Some(path)) => {
                    self.boot.settings.output_dir = path.display().to_string();
                    self.persist_settings(cx);
                }
                Job::Folder(None) => {}
            }
        }
        if self.boot.recording.phase == "recording" {
            self.boot.recording = self.engine.recording();
        }
        cx.notify();
    }

    fn apply(&mut self, result: Result<ActionResult, String>, cx: &mut Context<Self>) {
        match result {
            Ok(result) => {
                if let Some(mode) = result.mode.as_deref() {
                    self.mode = match mode {
                        "recording" => Mode::Recording,
                        "region" => Mode::Region,
                        _ => Mode::Browse,
                    };
                }
                self.notice = result.notice;
                self.warnings = result.warnings;
                self.boot.recording = result.recording;
                if let Some(item) = result.item {
                    self.boot.library.retain(|row| row.record.id != item.record.id);
                    self.boot.library.insert(0, item);
                }
                if let Some(id) = result.removed_id {
                    self.boot.library.retain(|row| row.record.id != id);
                }
                if self.mode == Mode::Browse {
                    self.preview = None;
                }
            }
            Err(message) => self.fail_text(message),
        }
        cx.notify();
    }

    fn fail_text(&mut self, message: String) {
        self.notice = Some(Notice { level: "error".into(), code: "UNKNOWN".into(), message });
        if self.mode == Mode::Preparing {
            self.mode = Mode::Browse;
        }
    }

    fn fire(&mut self, act: HotAct, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match act {
            HotAct::ShotScreen => self.spawn_screen(None, cx),
            HotAct::ShotWindow => self.spawn_windows("screenshot", cx),
            HotAct::ShotRegion => self.spawn_region(cx),
            HotAct::RecordToggle => self.spawn_toggle(cx),
            HotAct::RecordPause => self.spawn_pause(cx),
        }
    }

    fn spawn_screen(&mut self, monitor: Option<String>, cx: &mut Context<Self>) {
        self.busy = true;
        self.mode = Mode::Preparing;
        let engine = Arc::clone(&self.engine);
        let tx = self.tx.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(220));
            let result = engine.capture_screen(monitor).map_err(|e| e.message);
            let _ = tx.send(Job::Done(result));
        });
        cx.notify();
    }

    fn spawn_region(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.mode = Mode::Preparing;
        let engine = Arc::clone(&self.engine);
        let tx = self.tx.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(220));
            let result = engine.begin_region().map_err(|e| e.message);
            let _ = tx.send(Job::Region(result));
        });
        cx.notify();
    }

    fn spawn_windows(&mut self, intent: &str, cx: &mut Context<Self>) {
        let engine = Arc::clone(&self.engine);
        let tx = self.tx.clone();
        let intent = intent.to_string();
        thread::spawn(move || {
            let rows = engine.list_windows().unwrap_or_default();
            let _ = tx.send(Job::Windows { intent, rows });
        });
        cx.notify();
    }

    fn spawn_record_screen(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.mode = Mode::Preparing;
        let engine = Arc::clone(&self.engine);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = engine.start_screen(None).map_err(|e| e.message);
            let _ = tx.send(Job::Done(result));
        });
        cx.notify();
    }

    fn spawn_toggle(&mut self, cx: &mut Context<Self>) {
        if self.boot.recording.phase == "idle" {
            self.spawn_record_screen(cx);
            return;
        }
        self.busy = true;
        let engine = Arc::clone(&self.engine);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = engine.toggle().map_err(|e| e.message);
            let _ = tx.send(Job::Done(result));
        });
        cx.notify();
    }

    fn spawn_pause(&mut self, cx: &mut Context<Self>) {
        let engine = Arc::clone(&self.engine);
        let tx = self.tx.clone();
        let paused = self.boot.recording.phase == "paused";
        thread::spawn(move || {
            let result = if paused { engine.resume() } else { engine.pause() };
            let code = if paused { "RECORD_RESUMED" } else { "RECORD_PAUSED" };
            let _ = tx.send(Job::Status(result.map_err(|e| e.message), code));
        });
        cx.notify();
    }

    fn finish_region(&mut self, record: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((a, b)) = self.drag else {
            self.notice = Some(note(&self.boot.settings.locale, "error", "REGION_TOO_SMALL"));
            cx.notify();
            return;
        };
        let viewport = window.viewport_size();
        let (x0, y0) = map_point(a, viewport, self.preview_w, self.preview_h);
        let (x1, y1) = map_point(b, viewport, self.preview_w, self.preview_h);
        let engine = Arc::clone(&self.engine);
        let tx = self.tx.clone();
        self.mode = Mode::Preparing;
        self.preview = None;
        thread::spawn(move || {
            let result = engine
                .complete_region(x0, y0, x1 - x0, y1 - y0, record)
                .map_err(|e| e.message);
            let _ = tx.send(Job::Done(result));
        });
        cx.notify();
    }

    fn persist_settings(&mut self, cx: &mut Context<Self>) {
        match self.engine.save_settings(self.boot.settings.clone()) {
            Ok(boot) => {
                self.boot = boot;
                self.rebind();
            }
            Err(err) => self.notice = Some(Notice { level: "error".into(), code: err.code, message: err.message }),
        }
        cx.notify();
    }

    fn rebind(&mut self) {
        let Some(manager) = self.manager.take() else { return };
        let previous = std::mem::take(&mut self.registered);
        let _ = manager.unregister_all(&previous);
        self.hot_ids.clear();
        let hotkeys = self.boot.settings.hotkeys.clone();
        self.manager = Some(manager);
        let pairs = [
            (hotkeys.shot_screen, HotAct::ShotScreen),
            (hotkeys.shot_window, HotAct::ShotWindow),
            (hotkeys.shot_region, HotAct::ShotRegion),
            (hotkeys.record_toggle, HotAct::RecordToggle),
            (hotkeys.record_pause, HotAct::RecordPause),
        ];
        for (accel, act) in pairs {
            self.bind_hotkey(&accel, act);
        }
    }

    fn bind_hotkey(&mut self, accel: &str, act: HotAct) {
        let Some(manager) = self.manager.as_ref() else { return };
        let Ok(hotkey) = accel.parse::<HotKey>() else { return };
        let registered = manager.register(hotkey).is_ok();
        if registered {
            self.hot_ids.insert(hotkey.id(), act);
            self.registered.push(hotkey);
        }
    }


    fn render_shell(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = texts(&self.boot.settings.locale);
        let page = match self.page {
            Page::Home => 0,
            Page::Library => 1,
            Page::Settings => 2,
        };
        div()
            .flex()
            .size_full()
            .bg(rgb(0x12100e))
            .text_color(rgb(0xf4efe6))
            .child(self.sidebar(&t, page, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .h_full()
                    .id("content")
                    .overflow_y_scroll()
                    .p(px(28.))
                    .child(self.notice_bar())
                    .child(match self.page {
                        Page::Home => self.home(&t, cx).into_any_element(),
                        Page::Library => self.library(&t, cx).into_any_element(),
                        Page::Settings => self.settings(&t, cx).into_any_element(),
                    }),
            )
    }

    fn sidebar(&self, t: &Texts, page: usize, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(220.))
            .h_full()
            .bg(rgb(0x181511))
            .border_r_1()
            .border_color(rgb(0x2c261f))
            .flex()
            .flex_col()
            .p(px(18.))
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(34.))
                    .child(SharedString::from(format!("{} ·", t.name))),
            )
            .child(nav_button(t.home, page == 0, cx, |this, cx| {
                this.page = Page::Home;
                cx.notify();
            }))
            .child(nav_button(t.library, page == 1, cx, |this, cx| {
                this.page = Page::Library;
                cx.notify();
            }))
            .child(nav_button(t.settings, page == 2, cx, |this, cx| {
                this.page = Page::Settings;
                cx.notify();
            }))
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(0xb7ab9c))
                    .child(SharedString::from(t.byline)),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(0x8d8274))
                    .child(SharedString::from(format!("v{}", self.boot.version))),
            )
    }

    fn notice_bar(&self) -> impl IntoElement {
        let note = self.notice.clone();
        div().when_some(note, |this, note| {
            let color = if note.level == "error" { rgb(0xe36a55) } else { rgb(0xe4a15c) };
            this.mb(px(12.)).text_color(color).child(SharedString::from(note.message))
        })
    }

    fn home(&self, t: &Texts, cx: &mut Context<Self>) -> gpui::Div {
        let monitor = self.boot.monitors.first();
        let lead = monitor
            .map(|m| format!("{} · {}×{}", m.name, m.width, m.height))
            .unwrap_or_else(|| t.no_monitor.into());
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(div().text_size(px(13.)).text_color(rgb(0xb7ab9c)).child(SharedString::from(t.kicker)))
            .child(div().text_size(px(40.)).child(SharedString::from(t.home_title)))
            .child(div().text_color(rgb(0xb7ab9c)).child(SharedString::from(lead)))
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .child(self.card(t.screen, t.screen_body, cx, true))
                    .child(self.card(t.window, t.window_body, cx, false))
                    .child(self.card(t.region, t.region_body, cx, false)),
            )
            .child(div().mt(px(8.)).text_size(px(18.)).child(SharedString::from(t.devices)))
            .child(self.device_line(&t.screen_dev, &self.boot.report.screen.state))
            .child(self.device_line(&t.mic_dev, &self.boot.report.microphone.state))
            .child(self.device_line(&t.system_dev, &self.boot.report.system_audio.state))
            .child(self.device_line(&t.camera_dev, &self.boot.report.camera.state))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(0xb7ab9c))
                    .child(SharedString::from(t.bar_hint)),
            )
            .when(self.picker.is_some(), |this| this.child(self.picker_list(t, cx)))
    }

    fn card(&self, title: &'static str, body: &'static str, cx: &mut Context<Self>, screen: bool) -> impl IntoElement {
        let photo = texts(&self.boot.settings.locale).photo;
        let film = texts(&self.boot.settings.locale).film;
        div()
            .flex()
            .flex_col()
            .flex_1()
            .p(px(16.))
            .gap(px(8.))
            .bg(rgb(0x1e1a16))
            .border_1()
            .border_color(rgb(0x322b24))
            .rounded(px(16.))
            .min_h(px(210.))
            .child(div().text_size(px(24.)).child(SharedString::from(title)))
            .child(div().text_color(rgb(0xb7ab9c)).text_size(px(14.)).child(SharedString::from(body)))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(action_button(photo, false).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if screen {
                                this.spawn_screen(None, cx);
                            } else if title == texts(&this.boot.settings.locale).window {
                                this.spawn_windows("screenshot", cx);
                            } else {
                                this.spawn_region(cx);
                            }
                        }),
                    ))
                    .child(action_button(film, true).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            let kind = texts(&this.boot.settings.locale);
                            if title == kind.screen {
                                this.spawn_record_screen(cx);
                            } else if title == kind.window {
                                this.spawn_windows("recording", cx);
                            } else {
                                this.spawn_region(cx);
                            }
                        }),
                    )),
            )
    }

    fn picker_list(&self, t: &Texts, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.picker.as_ref().map(|(_, rows)| rows.clone()).unwrap_or_default();
        let intent = self.picker.as_ref().map(|(intent, _)| intent.clone()).unwrap_or_default();
        div()
            .mt(px(8.))
            .p(px(12.))
            .bg(rgb(0x181511))
            .rounded(px(12.))
            .child(div().mb(px(8.)).child(SharedString::from(t.pick_window)))
            .children(rows.into_iter().map(|row| {
                let id = row.id.clone();
                let intent = intent.clone();
                let label = format!("{} · {}×{}", row.title, row.width, row.height);
                div()
                    .py(px(6.))
                    .child(SharedString::from(label))
                    .cursor_pointer()
                    .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                        let id = id.clone();
                        let engine = Arc::clone(&this.engine);
                        let tx = this.tx.clone();
                        let recording = intent == "recording";
                        this.picker = None;
                        this.busy = true;
                        thread::spawn(move || {
                            let result = if recording {
                                engine.start_window(&id)
                            } else {
                                engine.capture_window(&id)
                            };
                            let _ = tx.send(Job::Done(result.map_err(|e| e.message)));
                        });
                        cx.notify();
                    }))
            }))
            .child(
                action_button(t.cancel, false).on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    this.picker = None;
                    cx.notify();
                })),
            )
    }

    fn device_line(&self, label: &str, state: &str) -> impl IntoElement {
        let locale = &self.boot.settings.locale;
        let state_label = match state {
            "ready" => if locale == "en" { "ready" } else { "disponible" },
            "missing" => if locale == "en" { "missing" } else { "absent" },
            "needsPermission" => if locale == "en" { "permission needed" } else { "à autoriser" },
            _ => if locale == "en" { "unavailable" } else { "indisponible" },
        };
        div()
            .text_size(px(14.))
            .text_color(rgb(0xb7ab9c))
            .child(SharedString::from(format!("{label} · {state_label}")))
    }

    fn library(&self, t: &Texts, cx: &mut Context<Self>) -> gpui::Div {
        let mut column = div().flex().flex_col().gap(px(12.)).child(
            div().text_size(px(32.)).child(SharedString::from(t.library)),
        );
        if self.boot.library.is_empty() {
            return column.child(div().text_color(rgb(0xb7ab9c)).child(SharedString::from(t.empty)));
        }
        for item in &self.boot.library {
            column = column.child(self.library_row(item, t, cx));
        }
        column
    }

    fn library_row(&self, item: &CaptureDto, t: &Texts, cx: &mut Context<Self>) -> impl IntoElement {
        let id = item.record.id.clone();
        let kind = if item.record.kind == "recording" { t.film } else { t.photo };
        let meta = format!(
            "{kind} · {}×{} · {}",
            item.record.width, item.record.height, item.record.filename
        );
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .p(px(10.))
            .bg(rgb(0x1e1a16))
            .rounded(px(12.))
            .child(div().flex_1().child(SharedString::from(meta)))
            .child(small_button(t.open).on_mouse_up(MouseButton::Left, cx.listener({
                let id = id.clone();
                move |this, _, _, cx| {
                    if let Err(err) = this.engine.open_capture(&id) {
                        this.notice = Some(notice_from(err));
                    }
                    cx.notify();
                }
            })))
            .child(small_button(t.reveal).on_mouse_up(MouseButton::Left, cx.listener({
                let id = id.clone();
                move |this, _, _, cx| {
                    if let Err(err) = this.engine.reveal_capture(&id) {
                        this.notice = Some(notice_from(err));
                    }
                    cx.notify();
                }
            })))
            .child(small_button(t.delete).on_mouse_up(MouseButton::Left, cx.listener({
                let id = id.clone();
                move |this, _, _, cx| {
                    match this.engine.delete_capture(&id) {
                        Ok(removed) => {
                            this.boot.library.retain(|row| row.record.id != removed);
                            this.notice = Some(note(&this.boot.settings.locale, "info", "DELETED"));
                        }
                        Err(err) => this.notice = Some(notice_from(err)),
                    }
                    cx.notify();
                }
            })))
    }

    fn settings(&self, t: &Texts, cx: &mut Context<Self>) -> gpui::Div {
        let s = &self.boot.settings;
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(div().text_size(px(32.)).child(SharedString::from(t.settings)))
            .child(div().text_color(rgb(0xb7ab9c)).child(SharedString::from(s.output_dir.clone())))
            .child(action_button(t.browse, true).on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| {
                let tx = this.tx.clone();
                thread::spawn(move || {
                    let folder = rfd::FileDialog::new().pick_folder();
                    let _ = tx.send(Job::Folder(folder));
                });
            })))
            .child(div().child(SharedString::from(t.format_hint)))
            .child(self.choice_row(t.height, &[
                (t.h_native, matches!(s.video_height, VideoHeight::Native)),
                (t.h720, matches!(s.video_height, VideoHeight::H720)),
                (t.h1080, matches!(s.video_height, VideoHeight::H1080)),
                (t.h1440, matches!(s.video_height, VideoHeight::H1440)),
                (t.h2160, matches!(s.video_height, VideoHeight::H2160)),
            ], cx, |this, index| {
                this.boot.settings.video_height = match index {
                    1 => VideoHeight::H720,
                    2 => VideoHeight::H1080,
                    3 => VideoHeight::H1440,
                    4 => VideoHeight::H2160,
                    _ => VideoHeight::Native,
                };
            }))
            .child(self.choice_row(t.fps, &[
                ("24", s.fps == 24),
                ("30", s.fps == 30),
                ("60", s.fps == 60),
            ], cx, |this, index| {
                this.boot.settings.fps = [24, 30, 60][index];
            }))
            .child(self.choice_row(t.quality, &[
                (t.q_high, matches!(s.quality, Quality::High)),
                (t.q_balanced, matches!(s.quality, Quality::Balanced)),
                (t.q_compact, matches!(s.quality, Quality::Compact)),
            ], cx, |this, index| {
                this.boot.settings.quality = match index {
                    0 => Quality::High,
                    2 => Quality::Compact,
                    _ => Quality::Balanced,
                };
            }))
            .child(toggle_row(t.cursor, s.show_cursor, cx, |this| this.boot.settings.show_cursor ^= true))
            .child(toggle_row(t.mic, s.capture_microphone, cx, |this| this.boot.settings.capture_microphone ^= true))
            .child(toggle_row(t.system, s.capture_system_audio, cx, |this| this.boot.settings.capture_system_audio ^= true))
            .child(toggle_row(t.camera, s.webcam_enabled, cx, |this| this.boot.settings.webcam_enabled ^= true))
            .child(self.choice_row(t.bubble, &[
                (t.small, matches!(s.webcam_size, WebcamSize::Small)),
                (t.medium, matches!(s.webcam_size, WebcamSize::Medium)),
                (t.large, matches!(s.webcam_size, WebcamSize::Large)),
            ], cx, |this, index| {
                this.boot.settings.webcam_size = [WebcamSize::Small, WebcamSize::Medium, WebcamSize::Large][index];
            }))
            .child(self.choice_row(t.corner, &[
                (t.top_left, matches!(s.webcam_position, Corner::TopLeft)),
                (t.top_right, matches!(s.webcam_position, Corner::TopRight)),
                (t.bottom_left, matches!(s.webcam_position, Corner::BottomLeft)),
                (t.bottom_right, matches!(s.webcam_position, Corner::BottomRight)),
            ], cx, |this, index| {
                this.boot.settings.webcam_position = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight][index];
            }))
            .child(div().text_color(rgb(0xb7ab9c)).child(SharedString::from(shortcut_line(&s.hotkeys))))
            .child(div().mt(px(8.)).text_size(px(18.)).child(SharedString::from(t.later)))
            .child(div().text_color(rgb(0xb7ab9c)).child(SharedString::from(t.later_body)))
            .child(action_button(t.lang, false).on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                this.boot.settings.locale = if this.boot.settings.locale == "en" { "fr".into() } else { "en".into() };
                this.persist_settings(cx);
            })))
    }

    fn choice_row(
        &self,
        label: &'static str,
        options: &[(&'static str, bool)],
        cx: &mut Context<Self>,
        apply: fn(&mut Kapture, usize),
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().w(px(140.)).child(SharedString::from(label)))
            .children(options.iter().enumerate().map(|(index, (name, on))| {
                let name = *name;
                action_button(name, *on).on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                    apply(this, index);
                    this.persist_settings(cx);
                }))
            }))
    }

    fn region_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = texts(&self.boot.settings.locale);
        let selection = self.drag.map(|(a, b)| {
            let ax = f32::from(a.x);
            let ay = f32::from(a.y);
            let bx = f32::from(b.x);
            let by = f32::from(b.y);
            let x = ax.min(bx);
            let y = ay.min(by);
            let w = (ax - bx).abs().max(1.);
            let h = (ay - by).abs().max(1.);
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .border_2()
                .border_color(rgb(0xe4a15c))
        });
        div()
            .size_full()
            .relative()
            .bg(rgb(0x000000))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                this.drag = Some((ev.position, ev.position));
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if let Some((start, _)) = this.drag {
                    this.drag = Some((start, ev.position));
                    cx.notify();
                }
            }))
            .when_some(self.preview.clone(), |this, image| {
                this.child(img(ImageSource::Render(image)).size_full().object_fit(gpui::ObjectFit::Fill))
            })
            .children(selection)
            .child(
                div()
                    .absolute()
                    .top(px(16.))
                    .left(px(16.))
                    .flex()
                    .gap(px(8.))
                    .child(div().text_color(rgb(0xf4efe6)).child(SharedString::from(t.drag_hint)))
                    .child(action_button(t.photo, false).on_mouse_up(MouseButton::Left, cx.listener(|this, _, window, cx| {
                        this.finish_region(false, window, cx);
                    })))
                    .child(action_button(t.film, true).on_mouse_up(MouseButton::Left, cx.listener(|this, _, window, cx| {
                        this.finish_region(true, window, cx);
                    })))
                    .child(action_button(t.cancel, false).on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| {
                        this.engine.cancel_region();
                        this.preview = None;
                        this.mode = Mode::Browse;
                        cx.notify();
                    }))),
            )
    }

    fn record_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = texts(&self.boot.settings.locale);
        let elapsed = format_ms(self.boot.recording.elapsed_ms);
        let paused = self.boot.recording.phase == "paused";
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(16.))
            .bg(rgb(0x12100e))
            .child(div().size(px(12.)).rounded(px(6.)).bg(rgb(0xe36a55)))
            .child(div().text_size(px(36.)).child(SharedString::from(elapsed)))
            .child(action_button(if paused { t.resume } else { t.pause }, false).on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.spawn_pause(cx)),
            ))
            .child(action_button(t.stop, true).on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.spawn_toggle(cx)),
            ))
    }
}

impl Render for Kapture {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.mode {
            Mode::Preparing => div()
                .size_full()
                .bg(rgb(0x12100e))
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(0xf4efe6))
                .child(SharedString::from(texts(&self.boot.settings.locale).preparing))
                .into_any_element(),
            Mode::Region => self.region_view(cx).into_any_element(),
            Mode::Recording => self.record_bar(cx).into_any_element(),
            Mode::Browse => self.render_shell(cx).into_any_element(),
        }
    }
}

fn nav_button(
    label: &'static str,
    active: bool,
    cx: &mut Context<Kapture>,
    f: fn(&mut Kapture, &mut Context<Kapture>),
) -> gpui::Div {
    let bg = if active { rgb(0x2a241c) } else { rgb(0x181511) };
    div()
        .px(px(10.))
        .py(px(8.))
        .rounded(px(10.))
        .bg(bg)
        .cursor_pointer()
        .child(SharedString::from(label))
        .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| f(this, cx)))
}

fn action_button(label: &'static str, filled: bool) -> gpui::Div {
    let mut el = div().px(px(12.)).py(px(7.)).rounded(px(999.)).cursor_pointer().child(SharedString::from(label));
    if filled {
        el = el.bg(rgb(0xe4a15c)).text_color(rgb(0x1a1208));
    } else {
        el = el.border_1().border_color(rgb(0x5c5348)).text_color(rgb(0xf4efe6));
    }
    el
}

fn small_button(label: &'static str) -> gpui::Div {
    action_button(label, false)
}

fn toggle_row(
    label: &'static str,
    on: bool,
    cx: &mut Context<Kapture>,
    flip: fn(&mut Kapture),
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(div().w(px(180.)).child(SharedString::from(label)))
        .child(action_button(if on { "on" } else { "off" }, on).on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                flip(this);
                this.persist_settings(cx);
            }),
        ))
}

fn map_point(point: Point<Pixels>, viewport: gpui::Size<Pixels>, w: u32, h: u32) -> (i32, i32) {
    let vw = f32::from(viewport.width).max(1.);
    let vh = f32::from(viewport.height).max(1.);
    let x = (f32::from(point.x) / vw * w as f32).round() as i32;
    let y = (f32::from(point.y) / vh * h as f32).round() as i32;
    (x, y)
}

fn rgba_image(image: &RgbaImage) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(smallvec![image::Frame::new(image.clone())]))
}

fn format_ms(ms: u64) -> String {
    let total = ms / 1000;
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn note(locale: &str, level: &str, code: &str) -> Notice {
    Notice { level: level.into(), code: code.into(), message: copy::text(locale, code) }
}

fn notice_from(err: CommandError) -> Notice {
    Notice { level: "error".into(), code: err.code, message: err.message }
}

fn shortcut_line(hotkeys: &Hotkeys) -> String {
    format!(
        "{}   {}   {}",
        hotkeys.shot_screen, hotkeys.shot_region, hotkeys.record_toggle
    )
}

struct Texts {
    name: &'static str,
    kicker: &'static str,
    byline: &'static str,
    home_title: &'static str,
    home: &'static str,
    library: &'static str,
    settings: &'static str,
    screen: &'static str,
    window: &'static str,
    region: &'static str,
    screen_body: &'static str,
    window_body: &'static str,
    region_body: &'static str,
    photo: &'static str,
    film: &'static str,
    devices: &'static str,
    screen_dev: &'static str,
    mic_dev: &'static str,
    system_dev: &'static str,
    camera_dev: &'static str,
    bar_hint: &'static str,
    no_monitor: &'static str,
    empty: &'static str,
    open: &'static str,
    reveal: &'static str,
    delete: &'static str,
    browse: &'static str,
    format_hint: &'static str,
    height: &'static str,
    fps: &'static str,
    quality: &'static str,
    h_native: &'static str,
    h720: &'static str,
    h1080: &'static str,
    h1440: &'static str,
    h2160: &'static str,
    q_high: &'static str,
    q_balanced: &'static str,
    q_compact: &'static str,
    cursor: &'static str,
    mic: &'static str,
    system: &'static str,
    camera: &'static str,
    bubble: &'static str,
    small: &'static str,
    medium: &'static str,
    large: &'static str,
    corner: &'static str,
    top_left: &'static str,
    top_right: &'static str,
    bottom_left: &'static str,
    bottom_right: &'static str,
    later: &'static str,
    later_body: &'static str,
    lang: &'static str,
    preparing: &'static str,
    drag_hint: &'static str,
    cancel: &'static str,
    pause: &'static str,
    resume: &'static str,
    stop: &'static str,
    pick_window: &'static str,
}

fn texts(locale: &str) -> Texts {
    if locale == "en" {
        Texts {
            name: "Kapture",
            kicker: "Kapture Desktop",
            byline: "An open-source project by Karom Studio.",
            home_title: "What do you want to keep?",
            home: "Home",
            library: "Library",
            settings: "Settings",
            screen: "Whole screen",
            window: "One window",
            region: "An area",
            screen_body: "Everything on this display.",
            window_body: "A single application.",
            region_body: "Draw the frame yourself.",
            photo: "Still",
            film: "Film",
            devices: "What Kapture can see and hear",
            screen_dev: "Screen",
            mic_dev: "Microphone",
            system_dev: "System audio",
            camera_dev: "Camera",
            bar_hint: "On a full-screen film, this window stays in the picture.",
            no_monitor: "No display detected.",
            empty: "Nothing saved yet.",
            open: "Open",
            reveal: "Show",
            delete: "Delete",
            browse: "Choose folder",
            format_hint: "Stills are PNG. Films are MP4, capped at 4K.",
            height: "Height",
            fps: "FPS",
            quality: "Quality",
            h_native: "Native",
            h720: "720p",
            h1080: "1080p",
            h1440: "1440p",
            h2160: "4K",
            q_high: "High",
            q_balanced: "Balanced",
            q_compact: "Compact",
            cursor: "Cursor in films",
            mic: "Microphone",
            system: "System audio",
            camera: "Camera bubble",
            bubble: "Bubble",
            small: "S",
            medium: "M",
            large: "L",
            corner: "Corner",
            top_left: "TL",
            top_right: "TR",
            bottom_left: "BL",
            bottom_right: "BR",
            later: "Later",
            later_body: "Arrows, text, blur, a quick crop, JPEG, WebP and sharing come next. Files stay on disk.",
            lang: "English / Français",
            preparing: "One moment…",
            drag_hint: "Drag to frame the area.",
            cancel: "Cancel",
            pause: "Pause",
            resume: "Resume",
            stop: "Stop",
            pick_window: "Choose a window",
        }
    } else {
        Texts {
            name: "Kapture",
            kicker: "Kapture Desktop",
            byline: "Un projet open source de Karom Studio.",
            home_title: "Que voulez-vous garder ?",
            home: "Accueil",
            library: "Bibliothèque",
            settings: "Réglages",
            screen: "Écran entier",
            window: "Une fenêtre",
            region: "Une zone",
            screen_body: "Tout ce qui est affiché sur cet écran.",
            window_body: "Une application, rien d'autre.",
            region_body: "Tracez le cadre à la main.",
            photo: "Photo",
            film: "Film",
            devices: "Ce que Kapture peut voir et entendre",
            screen_dev: "Écran",
            mic_dev: "Micro",
            system_dev: "Son du système",
            camera_dev: "Caméra",
            bar_hint: "En plein écran, cette fenêtre reste visible dans le film.",
            no_monitor: "Aucun écran détecté.",
            empty: "Aucune capture pour l'instant.",
            open: "Ouvrir",
            reveal: "Dossier",
            delete: "Supprimer",
            browse: "Choisir un dossier",
            format_hint: "Les photos sont en PNG. Les films sont en MP4, plafonnés à la 4K.",
            height: "Hauteur",
            fps: "Images/s",
            quality: "Qualité",
            h_native: "Native",
            h720: "720p",
            h1080: "1080p",
            h1440: "1440p",
            h2160: "4K",
            q_high: "Élevée",
            q_balanced: "Équilibrée",
            q_compact: "Compacte",
            cursor: "Curseur dans les films",
            mic: "Microphone",
            system: "Son du système",
            camera: "Bulle caméra",
            bubble: "Bulle",
            small: "S",
            medium: "M",
            large: "L",
            corner: "Coin",
            top_left: "HG",
            top_right: "HD",
            bottom_left: "BG",
            bottom_right: "BD",
            later: "Plus tard",
            later_body: "Flèches, texte, flou, recadrage, JPEG, WebP et partage viendront ensuite. Les fichiers restent sur le disque.",
            lang: "Français / English",
            preparing: "Un instant…",
            drag_hint: "Glissez pour encadrer la zone.",
            cancel: "Annuler",
            pause: "Pause",
            resume: "Reprendre",
            stop: "Stop",
            pick_window: "Choisir une fenêtre",
        }
    }
}

fn install_tray(tx: Sender<Job>) {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::TrayIconBuilder;
    let menu = Menu::new();
    let _ = menu.append(&MenuItem::with_id("shot-screen", "Capturer l'écran", true, None));
    let _ = menu.append(&MenuItem::with_id("shot-region", "Capturer une zone", true, None));
    let _ = menu.append(&MenuItem::with_id("record", "Enregistrer l'écran", true, None));
    let _ = menu.append(&MenuItem::with_id("quit", "Quitter", true, None));
    let icon = tray_icon_image();
    if let Ok(tray) = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Kapture Desktop").with_icon(icon).build() {
        Box::leak(Box::new((tray, tx)));
    }
    let _ = MenuEvent::receiver();
}

fn tray_loop(tx: Sender<Job>) {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, TrayIconBuilder};
    let menu = Menu::new();
    let _ = menu.append(&MenuItem::with_id("shot-screen", "Capturer l'écran", true, None));
    let _ = menu.append(&MenuItem::with_id("shot-region", "Capturer une zone", true, None));
    let _ = menu.append(&MenuItem::with_id("record", "Enregistrer l'écran", true, None));
    let _ = menu.append(&MenuItem::with_id("quit", "Quitter", true, None));
    let icon = tray_icon_image();
    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Kapture Desktop")
        .with_icon(icon)
        .build();
        loop {
        if let Ok(event) = MenuEvent::receiver().recv() {
            let act = match event.id.as_ref() {
                "shot-screen" => Some(HotAct::ShotScreen),
                "shot-region" => Some(HotAct::ShotRegion),
                "record" => Some(HotAct::RecordToggle),
                "quit" => {
                    std::process::exit(0);
                }
                _ => None,
            };
            if let Some(act) = act {
                let _ = tx.send(Job::Command(act));
            }
        }
    }
}

fn tray_icon_image() -> tray_icon::Icon {
    let mut rgba = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let dx = x as f32 - 15.5;
            let dy = y as f32 - 15.5;
            let d = (dx * dx + dy * dy).sqrt();
            let i = (y * 32 + x) * 4;
            if d < 15.0 {
                let ring = (d - 9.0).abs() < 2.4;
                if ring {
                    rgba[i] = 228;
                    rgba[i + 1] = 161;
                    rgba[i + 2] = 92;
                    rgba[i + 3] = 255;
                } else if d < 4.0 {
                    rgba[i] = 255;
                    rgba[i + 1] = 236;
                    rgba[i + 2] = 214;
                    rgba[i + 3] = 255;
                } else {
                    rgba[i] = 18;
                    rgba[i + 1] = 16;
                    rgba[i + 2] = 14;
                    rgba[i + 3] = 255;
                }
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, 32, 32).expect("icône")
}
