//! Runs the real app with no window, following a script, and saves frames as PNGs. This is
//! how the visuals and interactions get checked in tests and when working on the design.
//!
//! Usage: filetree [folder] --capture "size 1440 900; wait 2; shot out.png; click-name Photos"

use crate::app::{App, Key};
use crate::gpu::Gpu;
use crate::tree::NONE;
use std::path::PathBuf;
use std::io::Write as _;
use std::sync::{Arc, Mutex};

/// Frames being sent to ffmpeg, when a script is recording.
static RECORDER: Mutex<Option<std::process::Child>> = Mutex::new(None);

fn start_recording(width: u32, height: u32, path: &str) {
    let child = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"])
        .arg(format!("{width}x{height}"))
        .args(["-r", "60", "-i", "-", "-c:v", "libx264", "-preset", "medium", "-crf", "12", "-pix_fmt", "yuv420p"])
        .arg(path)
        .stdin(std::process::Stdio::piped())
        .spawn();
    match child {
        Ok(child) => *RECORDER.lock().unwrap() = Some(child),
        Err(err) => eprintln!("capture: couldn't start ffmpeg: {err}"),
    }
}

fn record_frame(gpu: &Gpu) {
    let mut guard = RECORDER.lock().unwrap();
    let Some(child) = guard.as_mut() else { return };
    let Some((_, _, mut rgba)) = gpu.read_pixels() else { return };
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(&rgba);
    }
}

fn stop_recording() {
    if let Some(mut child) = RECORDER.lock().unwrap().take() {
        drop(child.stdin.take());
        let _ = child.wait();
    }
}

/// Moves the pointer smoothly, the way a hand would, with the world running.
fn glide(app: &mut App, gpu: &mut Gpu, to: (f32, f32), seconds: f32) {
    let from = (app.pointer[0], app.pointer[1]);
    let steps = (seconds / STEP).round().max(1.0) as usize;
    // A slight arc keeps it from looking mechanical.
    let bow = ((to.0 - from.0).powi(2) + (to.1 - from.1).powi(2)).sqrt() * 0.06;
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let e = t * t * (3.0 - 2.0 * t);
        let lift = (t * std::f32::consts::PI).sin() * bow;
        app.on_pointer_move(from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e - lift);
        frame(app, gpu, STEP);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn screen_of(app: &App, name: &str) -> Option<(f32, f32)> {
    let id = app.find_by_name(name);
    if id == NONE {
        eprintln!("capture: no loaded node named {name}");
        return None;
    }
    let tip = app.tree.get(id).tip;
    let z = app.camera.zoom();
    Some((app.camera.ox.value + tip[0] * z, app.camera.oy.value - tip[1] * z))
}

/// Where a control is this frame, by what it does (written the way Debug prints it).
fn zone_center(app: &App, wanted: &str) -> Option<(f32, f32)> {
    let spot = app.zones.iter().rev().find(|z| format!("{:?}", z.action) == wanted).map(|z| z.rect);
    match spot {
        Some(r) => Some(((r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5)),
        None => {
            eprintln!("capture: no control {wanted} on screen");
            None
        }
    }
}

const STEP: f32 = 1.0 / 60.0;
static FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn run(start: Option<PathBuf>, script: &str) {
    let commands: Vec<Vec<String>> = script
        .split(';')
        .map(|c| c.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
        .filter(|c: &Vec<String>| !c.is_empty())
        .collect();

    let (mut width, mut height, mut scale) = (1440u32, 900u32, 1.0f32);
    for command in &commands {
        if command[0] == "size" {
            width = command[1].parse().unwrap_or(width);
            height = command[2].parse().unwrap_or(height);
            scale = command.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
        }
    }
    let mut gpu = Gpu::offscreen(width, height).expect("graphics device");
    let waker: crate::fs::Waker = Arc::new(|| {});
    let mut app = App::new(start, waker, width as f32, height as f32, scale);
    // A script must never read or replace what's on the real clipboard.
    app.acts.no_system_clipboard = true;
    frame(&mut app, &mut gpu, STEP);

    for command in &commands {
        let arg = |i: usize| command.get(i).map(String::as_str).unwrap_or("");
        let num = |i: usize| arg(i).parse::<f32>().unwrap_or(0.0);
        match command[0].as_str() {
            "size" => {}
            "wait" => advance(&mut app, &mut gpu, num(1)),
            "shot" => {
                frame(&mut app, &mut gpu, STEP);
                save(&gpu, arg(1));
            }
            "move" => {
                app.on_pointer_move(num(1), num(2));
                frame(&mut app, &mut gpu, STEP);
            }
            "click" => {
                app.on_pointer_move(num(1), num(2));
                frame(&mut app, &mut gpu, STEP);
                app.on_pointer_down();
                app.on_pointer_up();
            }
            "wheel" => {
                app.on_pointer_move(num(1), num(2));
                app.on_wheel(num(3));
            }
            "click-name" | "hover-name" => {
                let id = app.find_by_name(&command[1..].join(" "));
                if id == NONE {
                    eprintln!("capture: no loaded node named {}", command[1..].join(" "));
                    continue;
                }
                let tip = app.tree.get(id).tip;
                let z = app.camera.zoom();
                let x = app.camera.ox.value + tip[0] * z;
                let y = app.camera.oy.value - tip[1] * z;
                app.on_pointer_move(x, y);
                frame(&mut app, &mut gpu, STEP);
                if command[0] == "click-name" {
                    app.on_pointer_down();
                    app.on_pointer_up();
                }
            }
            "select" => {
                let id = app.find_by_name(&command[1..].join(" "));
                if id != NONE {
                    app.select(id, true);
                }
            }
            "expand" => {
                let id = app.find_by_name(&command[1..].join(" "));
                if id != NONE {
                    app.expand(id);
                }
            }
            "collapse" => {
                let id = app.find_by_name(&command[1..].join(" "));
                if id != NONE {
                    app.collapse(id);
                }
            }
            "goto" => {
                let path = PathBuf::from(command[1..].join(" "));
                app.navigate_to(path, true);
            }
            "type" => {
                for ch in command[1..].join(" ").chars() {
                    app.on_char(ch);
                }
            }
            "key" => {
                let key = match arg(1) {
                    "up" => Key::Up,
                    "down" => Key::Down,
                    "left" => Key::Left,
                    "right" => Key::Right,
                    "enter" => Key::Enter,
                    "space" => Key::Space,
                    "esc" => Key::Escape,
                    "back" => Key::Backspace,
                    "home" => Key::Home,
                    "find" => Key::Find,
                    "fit" => Key::Fit,
                    "hidden" => Key::ToggleHidden,
                    "altleft" => Key::AltLeft,
                    "altright" => Key::AltRight,
                    "reset" => Key::Reset,
                    "f2" => Key::Rename,
                    "del" => Key::Delete,
                    "end" => Key::End,
                    "undo" => Key::Undo,
                    "copy" => Key::CopyFiles,
                    "cut" => Key::CutFiles,
                    "paste" => Key::Paste,
                    "dup" => Key::Duplicate,
                    "newfolder" => Key::NewFolder,
                    "newfile" => Key::NewFile,
                    "all" => Key::SelectAll,
                    other => {
                        eprintln!("capture: unknown key {other}");
                        continue;
                    }
                };
                app.on_key(key);
            }
            "calm" => app.toggle_motion(),
            "ctrl" => app.acts.ctrl = arg(1) == "on",
            "shift" => app.acts.shift = arg(1) == "on",
            // Waits for file operations to finish and the tree to take in what they did.
            "ops" => {
                let mut seconds = 0.0;
                while app.acts.working.is_some() && seconds < 30.0 {
                    frame(&mut app, &mut gpu, STEP);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                    seconds += STEP;
                }
                advance(&mut app, &mut gpu, 0.6);
                println!("toasts: {:?}", app.acts.toasts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>());
            }
            // Chooses a row of the open context menu, the way a click would.
            "menu" => {
                let label = command[1..].join(" ");
                let row = app.acts.menu.as_ref().and_then(|m| m.items.iter().position(|i| i.label == label));
                match row {
                    Some(row) => app.on_action(crate::app::Action::MenuItem(row)),
                    None => eprintln!("capture: no menu row called {label}"),
                }
            }
            // Picks one thing up and drops it on another: drag-name Report.txt > Archive
            "drag-name" => {
                let text = command[1..].join(" ");
                let hold = text.ends_with(" hold");
                let text = text.trim_end_matches(" hold");
                let Some((from, to)) = text.split_once(" > ") else {
                    eprintln!("capture: drag-name needs 'from > to'");
                    continue;
                };
                let point = |app: &App, name: &str| {
                    let tip = app.tree.get(app.find_by_name(name)).tip;
                    let z = app.camera.zoom();
                    (app.camera.ox.value + tip[0] * z, app.camera.oy.value - tip[1] * z)
                };
                if app.find_by_name(from) == NONE || app.find_by_name(to) == NONE {
                    eprintln!("capture: can't drag {from} to {to}: one isn't loaded");
                    continue;
                }
                let (ax, ay) = point(&app, from);
                let (bx, by) = point(&app, to);
                app.on_pointer_move(ax, ay);
                frame(&mut app, &mut gpu, STEP);
                app.on_pointer_down();
                for i in 1..=12 {
                    let t = i as f32 / 12.0;
                    app.on_pointer_move(ax + (bx - ax) * t, ay + (by - ay) * t);
                    frame(&mut app, &mut gpu, STEP);
                }
                if !hold {
                    app.on_pointer_up();
                }
            }
            // Shows the Recycle Bin view with made-up contents, so checking its layout never
            // puts anyone's real deleted files in a picture.
            "sample-bin" => {
                app.acts.bin = Some(
                    (0..14)
                        .map(|i| crate::ops::BinItem {
                            name: format!("Old draft {i}.txt"),
                            was_at: PathBuf::from(format!(r"C:\Sample\Folder {i}\Old draft {i}.txt")),
                            deleted: 1_800_000_000 - i * 90_000,
                        })
                        .collect(),
                );
            }
            "bin-count" => match crate::ops::list_bin() {
                Ok(items) => println!("the bin can be read: {} items", items.len()),
                Err(err) => println!("the bin couldn't be read: {err}"),
            },
            // Brings one thing back from the Recycle Bin, to clean up after a test.
            "restore" => {
                let path = PathBuf::from(command[1..].join(" "));
                app.start_op(crate::ops::Request::Undo(crate::ops::Undo::Trashed(vec![path])));
            }
            "record" => {
                app.acts.film_cursor = true;
                start_recording(width, height, arg(1));
            }
            "stop-record" => stop_recording(),
            "mark" => println!("mark {} at frame {} ({:.3} s)", arg(1), FRAMES.load(std::sync::atomic::Ordering::Relaxed), FRAMES.load(std::sync::atomic::Ordering::Relaxed) as f32 / 60.0),
            // glide-name 0.6 Photos   /   glide-zone 0.5 ToastUndo(0)   /   glide-menu 0.4 Rename   /   glide 0.5 300 400
            "glide" => glide(&mut app, &mut gpu, (num(2), num(3)), num(1)),
            "glide-name" => {
                if let Some(to) = screen_of(&app, &command[2..].join(" ")) {
                    glide(&mut app, &mut gpu, to, num(1));
                }
            }
            "glide-zone" => {
                if let Some(to) = zone_center(&app, &command[2..].join(" ")) {
                    glide(&mut app, &mut gpu, to, num(1));
                }
            }
            "glide-menu" => {
                let label = command[2..].join(" ");
                let row = app.acts.menu.as_ref().and_then(|m| m.items.iter().position(|i| i.label == label));
                if let Some(to) = row.and_then(|row| zone_center(&app, &format!("MenuItem({row})"))) {
                    glide(&mut app, &mut gpu, to, num(1));
                }
            }
            // A press and release at the pointer, held long enough to see.
            "tap" => {
                app.on_pointer_down();
                advance(&mut app, &mut gpu, 0.08);
                app.on_pointer_up();
            }
            "rtap" => app.on_right_click(),
            "type-slow" => {
                for ch in command[1..].join(" ").chars() {
                    app.on_char(ch);
                    advance(&mut app, &mut gpu, 0.07);
                }
            }
            "release" => app.on_pointer_up(),
            "pointer-down" => app.on_pointer_down(),
            "click-zone" => {
                let wanted = command[1..].join(" ");
                let spot = app.zones.iter().rev().find(|z| format!("{:?}", z.action) == wanted).map(|z| z.rect);
                match spot {
                    Some(r) => {
                        app.on_pointer_move((r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5);
                        frame(&mut app, &mut gpu, STEP);
                        app.on_pointer_down();
                        app.on_pointer_up();
                    }
                    None => eprintln!("capture: no control {wanted} on screen"),
                }
            }
            "right-click" => {
                app.on_pointer_move(num(1), num(2));
                frame(&mut app, &mut gpu, STEP);
                app.on_right_click();
            }
            // Rests on a branch, then uses the real handle that appears on it.
            "handle-name" => {
                let id = app.find_by_name(&command[1..].join(" "));
                let tip = app.tree.get(id).tip;
                let z = app.camera.zoom();
                app.on_pointer_move(app.camera.ox.value + tip[0] * z, app.camera.oy.value - tip[1] * z);
                frame(&mut app, &mut gpu, STEP);
                frame(&mut app, &mut gpu, STEP);
                let spot = app.zones.iter().find(|z| matches!(z.action, crate::app::Action::CollapseBranch(_))).map(|z| z.rect);
                match spot {
                    Some(r) => {
                        app.on_pointer_move((r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5);
                        frame(&mut app, &mut gpu, STEP);
                    }
                    None => eprintln!("capture: no handle appeared on {}", command[1..].join(" ")),
                }
            }
            "press" => {
                app.on_pointer_down();
                app.on_pointer_up();
            }
            "right-click-name" => {
                let id = app.find_by_name(&command[1..].join(" "));
                let tip = app.tree.get(id).tip;
                let z = app.camera.zoom();
                app.on_pointer_move(app.camera.ox.value + tip[0] * z, app.camera.oy.value - tip[1] * z);
                frame(&mut app, &mut gpu, STEP);
                app.on_right_click();
            }
            "labels" => {
                for n in &app.tree.nodes {
                    if n.alive && n.name.starts_with(arg(1)) && n.label_alpha > 0.0 {
                        println!("{} slot {} alpha {:.2} off {:?}", n.name, n.label_slot, n.label_alpha, n.label_off);
                    }
                }
                println!("t={:.2} moving {} was {} dirty {}", app.time, app.layout_moving, app.was_moving, app.labels_dirty);
            }
            "camera" => {
                let z = app.camera.zoom();
                println!("origin {:.1},{:.1} zoom {:.3} (target zoom {:.3})", app.camera.ox.value, app.camera.oy.value, z, app.camera.log_zoom.target.exp());
                if app.selected != NONE {
                    let n = app.tree.get(app.selected);
                    println!(
                        "selected {} tip world {:?} screen {:.1},{:.1} target length {:.1} theta {:.3} grow {:.3}",
                        n.name,
                        n.tip,
                        app.camera.ox.value + n.tip[0] * z,
                        app.camera.oy.value - n.tip[1] * z,
                        n.a_length.target,
                        n.a_theta.target,
                        n.grow.value
                    );
                    let mut cur = n.parent;
                    while cur != NONE {
                        let p = app.tree.get(cur);
                        println!("  up: kind {:?} tip {:?} ext {:.1}/{:.1} sub {:.3} len {:.1}", p.kind, p.tip, p.ext_center, p.ext_radius, p.subscale, p.a_length.target);
                        cur = p.parent;
                    }
                }
            }
            "write" => {
                let path = command[1..].join(" ");
                if let Err(err) = std::fs::write(&path, "new") {
                    eprintln!("capture: couldn't write {path}: {err}");
                }
            }
            "remove" => {
                let path = command[1..].join(" ");
                if let Err(err) = std::fs::remove_file(&path) {
                    eprintln!("capture: couldn't remove {path}: {err}");
                }
            }
            "up" => app.go_up(),
            "stats" => {
                let alive = app.tree.nodes.iter().filter(|n| n.alive).count();
                println!(
                    "nodes alive {alive}, arena {}, visible {}, flowers {}, branches {}, labels {}, particles {}",
                    app.tree.nodes.len(),
                    app.visible.len(),
                    app.draw.flowers.len(),
                    app.draw.branches_long.len() + app.draw.branches_short.len(),
                    app.draw.labels.len(),
                    app.particles.len(),
                );
            }
            "settle" => {
                // Runs until nothing at all is moving, so a capture shows the true resting state.
                app.idle_for = 100.0;
                let mut seconds = 0.0;
                while frame(&mut app, &mut gpu, STEP) && seconds < 30.0 {
                    seconds += STEP;
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            "idle-check" => {
                // Runs until nothing is animating and reports how long that took.
                let mut seconds = 0.0;
                app.focused = true;
                loop {
                    let moving = frame(&mut app, &mut gpu, STEP);
                    seconds += STEP;
                    if !moving || seconds > 60.0 {
                        break;
                    }
                }
                println!("settled after {seconds:.2}s of animation");
            }
            "bench" => {
                let frames = num(1).max(1.0) as usize;
                let start = std::time::Instant::now();
                for _ in 0..frames {
                    app.wake_wind();
                    app.wind.snap(1.0);
                    frame(&mut app, &mut gpu, STEP);
                }
                let took = start.elapsed();
                println!("{frames} frames in {took:?}, {:.2} ms each", took.as_secs_f64() * 1000.0 / frames as f64);
            }
            other => eprintln!("capture: unknown command {other}"),
        }
    }
}

fn advance(app: &mut App, gpu: &mut Gpu, seconds: f32) {
    let steps = (seconds / STEP).round() as usize;
    for _ in 0..steps {
        frame(app, gpu, STEP);
        // Give the disk threads real time to answer.
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn frame(app: &mut App, gpu: &mut Gpu, dt: f32) -> bool {
    app.wall += dt;
    app.pump_events();
    app.redraw = false;
    let mut more = app.tick(dt);
    app.text.begin_frame();
    more |= app.build_scene(dt);
    app.build_ui();
    app.refresh_hover();
    if let Some((w, h, rgba)) = app.image_pending.take() {
        gpu.set_image(Some((w, h, &rgba)));
        app.image_clear = false;
    } else if app.image_clear {
        gpu.set_image(None);
        app.image_clear = false;
    }
    gpu.upload_glyphs(&mut app.text.uploads);
    app.text.atlas_reset = false;
    let _ = gpu.render(&app.draw);
    FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    record_frame(gpu);
    more || app.redraw
}

fn save(gpu: &Gpu, path: &str) {
    let Some((width, height, mut rgba)) = gpu.read_pixels() else {
        eprintln!("capture: couldn't read the frame back");
        return;
    };
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match image::RgbaImage::from_raw(width, height, rgba) {
        Some(img) => {
            if let Err(err) = img.save(path) {
                eprintln!("capture: couldn't save {path}: {err}");
            }
        }
        None => eprintln!("capture: frame had the wrong size"),
    }
}
