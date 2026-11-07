//! FileTree: a file browser that is a living tree.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![cfg_attr(not(feature = "capture"), allow(dead_code))]

mod actions;
mod app;
mod audio;
mod sounds;
#[cfg(feature = "capture")]
mod capture;
mod fs;
mod garden;
mod gpu;
mod layout;
mod ops;
mod overlay;
mod palette;
mod platform;
mod scene;
mod text;
mod tree;
mod ui;

use app::{App, Cursor, Key};
use gpu::{Gpu, RenderResult};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

struct Host {
    start: Option<PathBuf>,
    proxy: EventLoopProxy<()>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    app: Option<App>,
    last_frame: Instant,
    modifiers: ModifiersState,
    shown: bool,
    cursor: Cursor,
    started: Instant,
    measure_startup: bool,
    clipboard: Option<arboard::Clipboard>,
    /// When to hand unused memory back, once the app has gone still.
    trim_at: Option<Instant>,
    trimmed: bool,
    /// When only the breeze is moving, frames are paced instead of running flat out.
    paced_frame_at: Option<Instant>,
}

impl ApplicationHandler for Host {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if !matches!(cause, StartCause::ResumeTimeReached { .. }) {
            return;
        }
        let now = Instant::now();
        if self.paced_frame_at.is_some_and(|at| now >= at) {
            self.paced_frame_at = None;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        if self.trim_at.is_some_and(|at| now >= at) {
            self.trim_at = None;
            platform::trim_memory();
            self.trimmed = true;
        }
        self.schedule(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule(event_loop);
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        // A comfortable size, or as much of a small screen as fits.
        let (mut width, mut height) = (1320.0f64, 860.0f64);
        if let Some(monitor) = event_loop.primary_monitor() {
            let screen = monitor.size().to_logical::<f64>(monitor.scale_factor());
            width = width.min(screen.width * 0.94).max(760.0);
            height = height.min(screen.height * 0.88).max(480.0);
        }
        let attrs = Window::default_attributes()
            .with_title("FileTree")
            .with_inner_size(LogicalSize::new(width, height))
            .with_min_inner_size(LogicalSize::new(760.0, 480.0))
            // Stay hidden until the first frame is ready, so there's never a blank flash.
            .with_visible(false);
        let window = match event_loop.create_window(attrs) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                platform::tell_user(&format!("FileTree couldn't open a window.\n\n{err}"));
                event_loop.exit();
                return;
            }
        };
        let gpu = match Gpu::for_window(window.clone()) {
            Ok(gpu) => gpu,
            Err(err) => {
                platform::tell_user(&format!("FileTree couldn't start your graphics driver. Updating it usually fixes this.\n\n{err}"));
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        let proxy = self.proxy.clone();
        let waker: fs::Waker = Arc::new(move || {
            let _ = proxy.send_event(());
        });
        let mut app = App::new(
            self.start.take(),
            waker,
            size.width as f32,
            size.height as f32,
            window.scale_factor() as f32,
        );
        app.reduced_motion = platform::prefers_reduced_motion();
        // Scenery that never stops would keep the whole processor drawing it.
        app.garden.held_still = gpu.in_software;
        app.sounds.on = !platform::sound_off_marker().is_some_and(|marker| marker.exists());
        app.sounds.connect();
        self.gpu = Some(gpu);
        self.app = Some(app);
        self.window = Some(window.clone());
        self.last_frame = Instant::now();
        // Windows sends no paint events to a hidden window, so draw the first frame directly.
        self.frame(&window);
        window.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        if let (Some(app), Some(window)) = (&mut self.app, &self.window) {
            app.pump_events();
            if app.redraw {
                window.request_redraw();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let (Some(window), Some(app)) = (self.window.clone(), &mut self.app) else { return };
        // Anything done is timed against the real clock, even after a long still spell.
        app.wall = self.started.elapsed().as_secs_f32();
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                let scale = window.scale_factor() as f32;
                if let Some(app) = &mut self.app {
                    app.resize(size.width as f32, size.height as f32, scale);
                }
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let size = window.inner_size();
                if let Some(app) = &mut self.app {
                    app.resize(size.width as f32, size.height as f32, scale_factor as f32);
                }
                window.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                if let Some(app) = &mut self.app {
                    app.set_focused(focused);
                }
                self.paced_frame_at = None;
                window.request_redraw();
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
                if let Some(app) = &mut self.app {
                    app.acts.ctrl = self.modifiers.control_key();
                    app.acts.shift = self.modifiers.shift_key();
                    if app.acts.drag.is_some() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::DroppedFile(path) => {
                if let Some(app) = &mut self.app {
                    app.acts.dropped.push(path);
                    window.request_redraw();
                }
            }
            WindowEvent::HoveredFile(_) => {
                if let Some(app) = &mut self.app {
                    app.on_file_hovering();
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(app) = &mut self.app {
                    app.on_pointer_move(position.x as f32, position.y as f32);
                    if app.redraw {
                        window.request_redraw();
                    }
                }
                self.sync_cursor(&window);
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(app) = &mut self.app {
                    app.on_pointer_left();
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                if let Some(app) = &mut self.app {
                    // Clicks are timed against the real clock, even after a long still spell.
                    app.wall = self.started.elapsed().as_secs_f32();
                    match state {
                        ElementState::Pressed => app.on_pointer_down(),
                        ElementState::Released => app.on_pointer_up(),
                    }
                    window.request_redraw();
                }
                self.sync_cursor(&window);
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                if let Some(app) = &mut self.app {
                    app.on_right_click();
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Back, .. } => {
                if let Some(app) = &mut self.app {
                    app.go_back();
                    window.request_redraw();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Forward, .. } => {
                if let Some(app) = &mut self.app {
                    app.go_forward();
                    window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                if let Some(app) = &mut self.app {
                    app.on_wheel(lines);
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    self.on_key(&event.logical_key, event.text.as_deref());
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                self.frame(&window);
                self.schedule(event_loop);
            }
            _ => {}
        }
    }
}

impl Host {
    /// Sleeps until the next thing that's due, or indefinitely if nothing is.
    fn sync_cursor(&mut self, window: &Window) {
        let Some(app) = &self.app else { return };
        if app.cursor != self.cursor {
            self.cursor = app.cursor;
            window.set_cursor(match app.cursor {
                Cursor::Default => CursorIcon::Default,
                Cursor::Pointer => CursorIcon::Pointer,
                Cursor::Text => CursorIcon::Text,
                Cursor::Grabbing => CursorIcon::Grabbing,
                Cursor::Copy => CursorIcon::Copy,
                Cursor::NotAllowed => CursorIcon::NotAllowed,
            });
        }
    }

    fn schedule(&self, event_loop: &ActiveEventLoop) {
        let next = match (self.paced_frame_at, self.trim_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        event_loop.set_control_flow(match next {
            Some(at) => ControlFlow::WaitUntil(at),
            None => ControlFlow::Wait,
        });
    }

    fn on_key(&mut self, key: &WKey, text: Option<&str>) {
        let ctrl = self.modifiers.control_key() || self.modifiers.super_key();
        let alt = self.modifiers.alt_key();
        let shift = self.modifiers.shift_key();
        let Some(app) = &mut self.app else { return };
        let mapped = match key {
            WKey::Named(NamedKey::ArrowUp) if alt => Some(Key::AltUp),
            WKey::Named(NamedKey::ArrowLeft) if alt => Some(Key::AltLeft),
            WKey::Named(NamedKey::ArrowRight) if alt => Some(Key::AltRight),
            WKey::Named(NamedKey::ArrowUp) => Some(Key::Up),
            WKey::Named(NamedKey::ArrowDown) => Some(Key::Down),
            WKey::Named(NamedKey::ArrowLeft) => Some(Key::Left),
            WKey::Named(NamedKey::ArrowRight) => Some(Key::Right),
            WKey::Named(NamedKey::Enter) => Some(Key::Enter),
            WKey::Named(NamedKey::Space) if !app.search_active && app.acts.dialog.is_none() => Some(Key::Space),
            WKey::Named(NamedKey::Escape) => Some(Key::Escape),
            WKey::Named(NamedKey::Backspace) if ctrl => Some(Key::DeleteWord),
            WKey::Named(NamedKey::Backspace) => Some(Key::Backspace),
            WKey::Named(NamedKey::Home) => Some(Key::Home),
            WKey::Named(NamedKey::End) => Some(Key::End),
            WKey::Named(NamedKey::Delete) => Some(Key::Delete),
            WKey::Named(NamedKey::F2) => Some(Key::Rename),
            WKey::Named(NamedKey::PageUp) => Some(Key::PageUp),
            WKey::Named(NamedKey::PageDown) => Some(Key::PageDown),
            WKey::Character(c) if ctrl => match c.to_lowercase().as_str() {
                "f" | "l" => Some(Key::Find),
                "0" => Some(Key::Fit),
                "v" => Some(Key::Paste),
                "c" if shift => Some(Key::CopyPath),
                "c" => Some(Key::CopyFiles),
                "x" => Some(Key::CutFiles),
                "d" => Some(Key::Duplicate),
                "n" if shift => Some(Key::NewFolder),
                "n" => Some(Key::NewFile),
                "z" => Some(Key::Undo),
                "a" => Some(Key::SelectAll),
                "b" => Some(Key::TogglePanel),
                "m" => Some(Key::ToggleSound),
                "h" => Some(Key::ToggleHidden),
                "r" => Some(Key::Reset),
                _ => None,
            },
            _ => None,
        };
        if let Some(key) = mapped {
            app.on_key(key);
            return;
        }
        if !ctrl && !alt
            && let Some(text) = text
        {
            for ch in text.chars() {
                app.on_char(ch);
            }
        }
    }

    fn frame(&mut self, window: &Window) {
        let now = Instant::now();
        // A long gap (nothing was animating) must not count as one giant time step.
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(1.0 / 20.0);
        self.last_frame = now;
        let (Some(app), Some(gpu)) = (&mut self.app, &mut self.gpu) else { return };
        app.wall = self.started.elapsed().as_secs_f32();

        app.pump_events();
        app.redraw = false;
        let mut more = app.tick(dt);
        app.text.begin_frame();
        more |= app.build_scene(dt);
        app.build_ui();
        app.refresh_hover();

        if let Some(copy) = app.pending_copy.take() {
            if self.clipboard.is_none() {
                self.clipboard = arboard::Clipboard::new().ok();
            }
            if let Some(clip) = self.clipboard.as_mut() {
                let _ = clip.set_text(copy);
            }
        }
        if let Some((w, h, rgba)) = app.image_pending.take() {
            gpu.set_image(Some((w, h, &rgba)));
            app.image_clear = false;
        } else if app.image_clear {
            gpu.set_image(None);
            app.image_clear = false;
        }
        gpu.upload_glyphs(&mut app.text.uploads);
        if std::mem::take(&mut app.text.atlas_reset) {
            more = true;
        }
        match gpu.render(&app.draw) {
            RenderResult::Reconfigure => {
                let size = window.inner_size();
                gpu.resize(size.width, size.height);
                more = true;
            }
            RenderResult::Skipped | RenderResult::Drawn => {}
        }
        if std::mem::take(&mut self.trimmed) && self.measure_startup {
            eprintln!("first frame after trimming took {:?}", now.elapsed());
        }
        if !self.shown {
            self.shown = true;
            window.set_visible(true);
            if self.measure_startup {
                eprintln!("first frame after {:?}", self.started.elapsed());
            }
        }
        self.paced_frame_at = None;
        if more || app.redraw {
            self.trim_at = None;
            if app.only_breeze && !app.redraw {
                self.paced_frame_at = Some(now + Duration::from_millis(33));
            } else {
                window.request_redraw();
            }
        } else {
            // Handing memory back makes the next frame slower, so wait for a real pause.
            let wait = if app.focused { 30 } else { 4 };
            self.trim_at = Some(now + Duration::from_secs(wait));
            // Messages still have to leave on time, even with nothing else going on.
            if let Some(due) = app.next_due() {
                self.paced_frame_at = Some(now + Duration::from_secs_f32(due));
            }
        }
        let cursor = app.cursor;
        if cursor != self.cursor {
            self.sync_cursor(window);
        }
    }
}

fn start_path(args: &[String]) -> Option<PathBuf> {
    // With no folder given, the tree starts from the disks themselves.
    let path = args.iter().find(|a| !a.is_empty() && !a.starts_with("--")).map(PathBuf::from)?;
    // Resolve relative paths, without the \\?\ form that Windows would otherwise add.
    let absolute = std::path::absolute(&path).unwrap_or(path);
    Some(absolute)
}

fn main() {
    let started = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("FileTree: a file browser that is a living tree.\n\nUsage: filetree [folder]");
        return;
    }
    #[cfg(feature = "capture")]
    if let Some(at) = args.iter().position(|a| a == "--capture") {
        let script = args.get(at + 1).cloned().unwrap_or_default();
        let rest: Vec<String> = args.iter().enumerate().filter(|(i, _)| *i != at && *i != at + 1).map(|(_, a)| a.clone()).collect();
        capture::run(start_path(&rest), &script);
        return;
    }

    let event_loop = match EventLoop::<()>::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(err) => {
            platform::tell_user(&format!("FileTree couldn't start.\n\n{err}"));
            return;
        }
    };
    // Sleep until something happens: with nothing moving, the app uses no CPU at all.
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut host = Host {
        start: start_path(&args),
        proxy: event_loop.create_proxy(),
        window: None,
        gpu: None,
        app: None,
        last_frame: Instant::now(),
        modifiers: ModifiersState::empty(),
        shown: false,
        cursor: Cursor::Default,
        started,
        measure_startup: args.iter().any(|a| a == "--startup-time"),
        clipboard: None,
        trim_at: None,
        trimmed: false,
        paced_frame_at: None,
    };
    if let Err(err) = event_loop.run_app(&mut host) {
        platform::tell_user(&format!("FileTree stopped unexpectedly.\n\n{err}"));
    }
}
