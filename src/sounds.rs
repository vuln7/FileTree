//! The small sounds of handling the tree: paper unfolding, wood stretching, leaves stirring.
//!
//! Every sound is worked out from plain maths the moment it's asked for, so no two are quite the
//! same and the app carries no recordings. They answer what you do and nothing else, so there's
//! no background sound. The sound device is opened when you reach for something and closed again
//! a few seconds after the last sound, so an app at rest isn't holding on to it.

use std::f32::consts::{PI, TAU};
use std::num::NonZero;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

const RATE: f32 = 44100.0;
/// All the sounds together, in decibels. Each is levelled against the others below.
const VOLUME: f32 = 6.0;
/// How long the device stays open after a sound, so the next one doesn't have to wait for it.
const KEEP_OPEN: Duration = Duration::from_secs(6);
/// After failing to open a device, how long to leave it before trying again.
const RETRY_AFTER: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cue {
    /// A fingertip on a piece of paper: choosing something.
    Pick,
    /// The lightest touch: a button or a menu.
    Tick,
    /// A flower's petals unfolding. The number picks its note, so a folder opening is a small
    /// rising run.
    Bloom(u32),
    /// A branch reaching out.
    Grow,
    /// A branch drawing back in.
    Close,
    /// Leaves stirring, from 0 barely to 1 the whole tree.
    Rustle(f32),
    /// A flower peeled up off the page.
    Lift,
    /// A flower set down on a branch, which shakes a little.
    Land,
    /// One sheet of paper sliding across another.
    Slide,
    /// Petals coming away and drifting down.
    Shed,
    /// Something coming back.
    Regrow,
    /// A dull knock on wood: that didn't work.
    Knock,
}

impl Cue {
    /// The shortest gap between two of the same sound, so a burst never turns into a rattle.
    fn min_gap(self) -> f32 {
        match self {
            Cue::Pick | Cue::Tick => 0.05,
            Cue::Bloom(_) => 0.0,
            Cue::Grow | Cue::Close => 0.11,
            Cue::Rustle(_) => 0.6,
            _ => 0.2,
        }
    }

    fn slot(self) -> usize {
        match self {
            Cue::Pick => 0,
            Cue::Tick => 1,
            Cue::Bloom(_) => 2,
            Cue::Grow => 3,
            Cue::Close => 4,
            Cue::Rustle(_) => 5,
            Cue::Lift => 6,
            Cue::Land => 7,
            Cue::Slide => 8,
            Cue::Shed => 9,
            Cue::Regrow => 10,
            Cue::Knock => 11,
        }
    }
}

enum Message {
    /// Someone's about to do something: have the device ready.
    Warm,
    Play { cue: Cue, seed: u32, pan: f32, gain: f32, delay: f32 },
}

pub struct Sounds {
    /// Whether the player wants sound at all.
    pub on: bool,
    sender: Option<Sender<Message>>,
    last: [f32; 12],
    count: u32,
    /// What's been asked for, for the tests to read.
    #[cfg(test)]
    pub heard: Vec<Cue>,
    /// Everything played while a capture records, to make its soundtrack from.
    #[cfg(feature = "capture")]
    pub log: Option<Vec<Logged>>,
}

/// One sound played during a capture.
#[cfg(feature = "capture")]
pub struct Logged {
    pub at: f32,
    pub cue: Cue,
    pub seed: u32,
    pub pan: f32,
    pub gain: f32,
}

impl Default for Sounds {
    /// Silent until the host connects it to a real device, so tests and captures make no noise.
    fn default() -> Self {
        Sounds {
            on: true,
            sender: None,
            last: [-100.0; 12],
            count: 0,
            #[cfg(test)]
            heard: Vec::new(),
            #[cfg(feature = "capture")]
            log: None,
        }
    }
}

impl Sounds {
    /// Starts the thread that plays sounds. It sleeps until there's one to play.
    pub fn connect(&mut self) {
        let (sender, receiver) = channel();
        let started = std::thread::Builder::new().name("sounds".into()).spawn(move || player(receiver));
        if started.is_ok() {
            self.sender = Some(sender);
        }
    }

    pub fn is_connected(&self) -> bool {
        self.sender.is_some()
    }

    pub fn warm(&self) {
        if self.on
            && let Some(sender) = &self.sender
        {
            let _ = sender.send(Message::Warm);
        }
    }

    /// Plays a sound unless it's too soon after the last of its kind. `now` is in seconds.
    pub fn play(&mut self, cue: Cue, now: f32, pan: f32, gain: f32, delay: f32) {
        if !self.on {
            return;
        }
        let slot = cue.slot();
        if now - self.last[slot] < cue.min_gap() {
            return;
        }
        self.last[slot] = now;
        self.count = self.count.wrapping_add(1);
        #[cfg(test)]
        self.heard.push(cue);
        let seed = self.count.wrapping_mul(2_654_435_761) ^ (now * 1000.0) as u32;
        #[cfg(feature = "capture")]
        if let Some(log) = &mut self.log {
            log.push(Logged { at: now + delay, cue, seed, pan, gain });
        }
        if let Some(sender) = &self.sender {
            let _ = sender.send(Message::Play { cue, seed, pan, gain, delay });
        }
    }
}

fn player(receiver: Receiver<Message>) {
    let mut device: Option<rodio::MixerDeviceSink> = None;
    let mut close_at = Instant::now();
    let mut retry_at = Instant::now();
    loop {
        let message = if device.is_some() {
            match receiver.recv_timeout(close_at.saturating_duration_since(Instant::now())) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => {
                    device = None;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match receiver.recv() {
                Ok(message) => message,
                Err(_) => return,
            }
        };
        if device.is_none() && Instant::now() >= retry_at {
            match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(mut sink) => {
                    sink.log_on_drop(false);
                    device = Some(sink);
                }
                // No device right now: not worth bothering anyone about.
                Err(_) => retry_at = Instant::now() + RETRY_AFTER,
            }
        }
        let Some(sink) = &device else { continue };
        let mut hold = KEEP_OPEN;
        if let Message::Play { cue, seed, pan, gain, delay } = message {
            let samples: Vec<f32> = render(cue, seed, pan).iter().map(|s| s * gain).collect();
            let seconds = samples.len() as f32 / 2.0 / RATE + delay;
            hold += Duration::from_secs_f32(seconds);
            let buffer = rodio::buffer::SamplesBuffer::new(NonZero::new(2).unwrap(), NonZero::new(RATE as u32).unwrap(), samples);
            if delay > 0.0 {
                sink.mixer().add(rodio::source::Source::delay(buffer, Duration::from_secs_f32(delay)));
            } else {
                sink.mixer().add(buffer);
            }
        }
        close_at = close_at.max(Instant::now() + hold);
    }
}

// ---------------------------------------------------------------- the maths

struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B1) | 1)
    }

    fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16_777_216.0
    }

    fn noise(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    fn between(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }
}

/// A resonant filter that stays stable however its frequency moves.
#[derive(Default, Clone, Copy)]
struct Filter {
    a: f32,
    b: f32,
}

impl Filter {
    /// Returns (low, band, high).
    fn run(&mut self, x: f32, hz: f32, q: f32) -> (f32, f32, f32) {
        let g = (PI * hz.clamp(20.0, 18000.0) / RATE).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.b;
        let v1 = a1 * self.a + a2 * v3;
        let v2 = self.b + a2 * self.a + a3 * v3;
        self.a = 2.0 * v1 - self.a;
        self.b = 2.0 * v2 - self.b;
        (v2, v1, x - k * v1 - v2)
    }
}

/// Up over `rise` and back down by 1, for a position from 0 to 1.
fn hump(t: f32, rise: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 {
        return 0.0;
    }
    if t < rise {
        (t / rise * PI * 0.5).sin().powi(2)
    } else {
        (((t - rise) / (1.0 - rise)) * PI * 0.5).cos().powi(2)
    }
}

fn empty(seconds: f32) -> Vec<f32> {
    vec![0.0; (seconds * RATE) as usize]
}

/// One crackle of paper or leaf: a tiny burst of filtered noise.
fn grain(out: &mut [f32], at: f32, hz: f32, decay: f32, amp: f32, rng: &mut Rng) {
    let start = (at * RATE) as usize;
    let len = (decay * 6.0 * RATE) as usize;
    let mut filter = Filter::default();
    let onset = (0.0003 * RATE).max(1.0);
    for i in 0..len {
        let Some(slot) = out.get_mut(start + i) else { break };
        let t = i as f32;
        let envelope = (t / onset).min(1.0) * (-t / (decay * RATE)).exp();
        let (_, band, _) = filter.run(rng.noise() * envelope, hz, 1.4);
        *slot += band * amp;
    }
}

/// A scatter of crackles, thick where `density` is high.
fn crackle(out: &mut [f32], rng: &mut Rng, span: (f32, f32), density: impl Fn(f32) -> f32, hz: (f32, f32), decay: (f32, f32), amp: f32) {
    let step = 0.001;
    let mut t = span.0;
    while t < span.1 {
        let position = (t - span.0) / (span.1 - span.0);
        if rng.unit() < density(position) * step {
            let loud = rng.unit();
            let hz = rng.between(hz.0, hz.1);
            let decay = rng.between(decay.0, decay.1);
            // Anywhere in this step, or the crackles fall on a grid and it buzzes like a ratchet.
            let at = t + rng.unit() * step;
            // Mostly small, now and then a bigger one, the way paper really crackles.
            grain(out, at, hz, decay, amp * (0.2 + 0.8 * loud * loud * loud), rng);
        }
        t += step;
    }
}

/// Breathy air whose pitch and level follow the shapes given, for a position from 0 to 1.
fn hush(out: &mut [f32], rng: &mut Rng, span: (f32, f32), hz: impl Fn(f32) -> f32, level: impl Fn(f32) -> f32, q: f32) {
    let start = (span.0 * RATE) as usize;
    let len = ((span.1 - span.0) * RATE) as usize;
    let mut filter = Filter::default();
    for i in 0..len {
        let Some(slot) = out.get_mut(start + i) else { break };
        let position = i as f32 / len as f32;
        let (_, band, _) = filter.run(rng.noise(), hz(position), q);
        *slot += band * level(position);
    }
}

/// A struck resonance dying away: the body of a tap, or a piece of wood.
fn ring(out: &mut [f32], at: f32, hz: f32, decay: f32, amp: f32) {
    let start = (at * RATE) as usize;
    let len = (decay * 7.0 * RATE) as usize;
    for i in 0..len {
        let Some(slot) = out.get_mut(start + i) else { break };
        let t = i as f32 / RATE;
        let attack = (t / 0.0015).min(1.0);
        *slot += (TAU * hz * t).sin() * (-t / decay).exp() * attack * amp;
    }
}

/// The stick and slip of wood being bent: a run of tiny knocks through its resonances.
fn creak(out: &mut [f32], rng: &mut Rng, span: (f32, f32), rate: (f32, f32), body: f32, level: impl Fn(f32) -> f32) {
    let mut t = span.0;
    while t < span.1 {
        let position = (t - span.0) / (span.1 - span.0);
        let amp = level(position) * rng.between(0.45, 1.0);
        let wobble = rng.between(0.97, 1.03);
        ring(out, t, body * wobble, 0.006, amp);
        ring(out, t, body * 2.53 * wobble, 0.0035, amp * 0.55);
        ring(out, t, body * 4.87 * wobble, 0.002, amp * 0.3);
        let now = rate.0 + (rate.1 - rate.0) * position;
        t += rng.between(0.7, 1.3) / now;
    }
}

const NOTES: [f32; 6] = [349.2, 392.0, 440.0, 523.3, 587.3, 698.5];

/// Lays the sound in a small space, brings it to the level asked for, and fades its ends.
fn finish(mono: Vec<f32>, peak_db: f32, pan: f32) -> Vec<f32> {
    let tail = (0.09 * RATE) as usize;
    let mut left = vec![0.0f32; mono.len() + tail];
    let mut right = vec![0.0f32; mono.len() + tail];
    // Keep the rumble out, and the very top soft, as if it were a little way off.
    let mut clean = mono;
    let mut high = Filter::default();
    let mut low = Filter::default();
    for s in &mut clean {
        let (_, _, without_rumble) = high.run(*s, 70.0, 0.7);
        let (soft, _, _) = low.run(without_rumble, 11000.0, 0.7);
        *s = soft;
    }
    let (pan_left, pan_right) = (((1.0 - pan) * 0.5).sqrt() * std::f32::consts::SQRT_2, ((1.0 + pan) * 0.5).sqrt() * std::f32::consts::SQRT_2);
    for (i, s) in clean.iter().enumerate() {
        left[i] += s * pan_left;
        right[i] += s * pan_right;
    }
    // A few quiet early reflections, different each side.
    for (ms_left, ms_right, level) in [(11.3f32, 13.7f32, 0.13f32), (19.1, 23.3, 0.09), (31.7, 37.9, 0.06), (53.0, 47.0, 0.035)] {
        let (a, b) = ((ms_left * 0.001 * RATE) as usize, (ms_right * 0.001 * RATE) as usize);
        let mut soft = Filter::default();
        for (i, s) in clean.iter().enumerate() {
            let (dull, _, _) = soft.run(*s, 3200.0, 0.7);
            left[i + a] += dull * level;
            right[i + b] += dull * level;
        }
    }
    let peak = left.iter().chain(right.iter()).fold(0.0f32, |m, s| m.max(s.abs())).max(0.000_001);
    let gain = 10f32.powf((peak_db + VOLUME) / 20.0) / peak;
    let len = left.len();
    let fade = (0.02 * RATE) as usize;
    let mut out = Vec::with_capacity(len * 2);
    for i in 0..len {
        let edge = ((len - i) as f32 / fade as f32).min(1.0);
        out.push(left[i] * gain * edge);
        out.push(right[i] * gain * edge);
    }
    out
}

/// Works out one sound as interleaved stereo samples. `pan` runs from -1 left to 1 right.
pub fn render(cue: Cue, seed: u32, pan: f32) -> Vec<f32> {
    let mut rng = Rng::new(seed);
    let r = &mut rng;
    let (mono, peak_db) = match cue {
        Cue::Pick => {
            let mut o = empty(0.09);
            ring(&mut o, 0.0, r.between(230.0, 300.0), 0.016, 0.8);
            grain(&mut o, 0.0, r.between(1500.0, 2100.0), 0.0022, 2.2, r);
            grain(&mut o, 0.004, r.between(3200.0, 4400.0), 0.0012, 0.7, r);
            (o, -24.0)
        }
        Cue::Tick => {
            let mut o = empty(0.05);
            ring(&mut o, 0.0, r.between(380.0, 460.0), 0.007, 0.6);
            grain(&mut o, 0.0, r.between(2200.0, 3000.0), 0.0012, 1.6, r);
            (o, -31.0)
        }
        Cue::Bloom(note) => {
            let mut o = empty(0.34);
            // The sheet of petals opening: a breath of air that brightens, with paper crackling.
            hush(&mut o, r, (0.0, 0.3), |t| 500.0 + 1900.0 * t, |t| hump(t, 0.6) * 0.42, 0.8);
            crackle(&mut o, r, (0.03, 0.27), |t| 130.0 * hump(t, 0.6), (1700.0, 4600.0), (0.001, 0.003), 0.8);
            // Under it, only a hint of a wooden note.
            let hz = NOTES[note as usize % NOTES.len()];
            ring(&mut o, 0.05, hz, 0.06, 0.07);
            (o, -24.0)
        }
        Cue::Grow => {
            let mut o = empty(0.56);
            hush(&mut o, r, (0.0, 0.5), |t| 420.0 + 1500.0 * t * t, |t| hump(t, 0.5) * 0.3, 0.9);
            let body = r.between(280.0, 380.0);
            creak(&mut o, r, (0.04, 0.4), (14.0, 34.0), body, |t| hump(t, 0.55) * 0.13);
            crackle(&mut o, r, (0.1, 0.5), |t| 130.0 * hump(t, 0.45), (2400.0, 6400.0), (0.0006, 0.0022), 0.8);
            (o, -22.0)
        }
        Cue::Close => {
            let mut o = empty(0.3);
            hush(&mut o, r, (0.0, 0.24), |t| 1500.0 - 1050.0 * t, |t| hump(t, 0.2) * 0.3, 0.9);
            let body = r.between(300.0, 400.0);
            creak(&mut o, r, (0.01, 0.17), (36.0, 18.0), body, |t| hump(t, 0.25) * 0.1);
            crackle(&mut o, r, (0.0, 0.16), |t| 90.0 * hump(t, 0.3), (2200.0, 5200.0), (0.0006, 0.0018), 0.6);
            // Folded away: a soft touch of paper at the end.
            ring(&mut o, 0.19, r.between(210.0, 260.0), 0.014, 0.28);
            (o, -25.0)
        }
        Cue::Rustle(strength) => {
            let strength = strength.clamp(0.0, 1.0);
            let length = 0.65 + 0.6 * strength;
            let mut o = empty(length);
            // A few flurries, each a little different, the way a gust moves through a tree.
            let flurries = 2 + (strength * 2.0) as usize;
            for i in 0..flurries {
                let at = i as f32 / flurries as f32 * length * 0.62 + r.between(0.0, 0.05);
                let long = r.between(0.24, 0.36) + 0.14 * strength;
                let thick = r.between(420.0, 620.0) + 380.0 * strength;
                let end = (at + long).min(length);
                // Leaves flutter: the crackle thickens and thins many times a second.
                let (flutter, phase) = (r.between(9.0, 17.0), r.between(0.0, TAU));
                let seconds = end - at;
                let thickness = |t: f32| thick * hump(t, 0.4) * (0.62 + 0.38 * (t * seconds * flutter * TAU + phase).sin());
                crackle(&mut o, r, (at, end), thickness, (1900.0, 8600.0), (0.0005, 0.0036), 0.85);
                hush(&mut o, r, (at, end), |_| 3000.0, |t| hump(t, 0.4) * 0.1, 0.5);
                hush(&mut o, r, (at, end), |_| 800.0, |t| hump(t, 0.4) * 0.04, 0.7);
            }
            (o, -32.0 + strength * 9.0)
        }
        Cue::Lift => {
            let mut o = empty(0.18);
            hush(&mut o, r, (0.0, 0.14), |t| 900.0 + 2200.0 * t, |t| hump(t, 0.65) * 0.3, 0.9);
            crackle(&mut o, r, (0.0, 0.1), |t| 130.0 * hump(t, 0.5), (2800.0, 6000.0), (0.0006, 0.0016), 0.9);
            (o, -27.0)
        }
        Cue::Land => {
            let mut o = empty(0.72);
            // It lands: a soft weight on the wood.
            ring(&mut o, 0.0, 118.0, 0.05, 0.75);
            ring(&mut o, 0.0, r.between(240.0, 290.0), 0.017, 0.5);
            grain(&mut o, 0.0, 1700.0, 0.0024, 1.6, r);
            // And the branch shakes its leaves.
            crackle(&mut o, r, (0.03, 0.62), |t| 420.0 * hump(t, 0.25), (2000.0, 8000.0), (0.0005, 0.003), 0.5);
            hush(&mut o, r, (0.02, 0.62), |_| 3400.0, |t| hump(t, 0.2) * 0.035, 0.6);
            (o, -21.0)
        }
        Cue::Slide => {
            let mut o = empty(0.3);
            hush(&mut o, r, (0.0, 0.28), |t| 1300.0 + 1300.0 * t, |t| hump(t, 0.3) * 0.3, 0.7);
            crackle(&mut o, r, (0.02, 0.24), |t| 55.0 * hump(t, 0.4), (3000.0, 6000.0), (0.0006, 0.0015), 0.4);
            (o, -29.0)
        }
        Cue::Shed => {
            let mut o = empty(1.0);
            let mut at = 0.0;
            let mut air = 3200.0;
            let mut level = 1.0;
            // Each petal comes away a little later and a little lower.
            for _ in 0..6 {
                let long = r.between(0.09, 0.15);
                hush(&mut o, r, (at, at + long), |t| air * (1.0 - 0.3 * t), |t| hump(t, 0.25) * 0.26 * level, 0.9);
                crackle(&mut o, r, (at, at + long * 0.7), |t| 120.0 * hump(t, 0.3), (air * 0.8, air * 1.6), (0.0006, 0.0015), 0.7 * level);
                at += r.between(0.11, 0.17);
                air *= 0.83;
                level *= 0.82;
            }
            (o, -25.0)
        }
        Cue::Regrow => {
            let mut o = empty(0.42);
            hush(&mut o, r, (0.0, 0.3), |t| 500.0 + 2600.0 * t * t, |t| hump(t, 0.7) * 0.3, 0.9);
            crackle(&mut o, r, (0.1, 0.3), |t| 110.0 * hump(t, 0.6), (2600.0, 6000.0), (0.0008, 0.002), 0.9);
            ring(&mut o, 0.24, NOTES[2], 0.07, 0.13);
            ring(&mut o, 0.29, NOTES[4], 0.075, 0.13);
            (o, -24.0)
        }
        Cue::Knock => {
            let mut o = empty(0.14);
            ring(&mut o, 0.0, r.between(165.0, 185.0), 0.024, 0.8);
            ring(&mut o, 0.0, r.between(430.0, 480.0), 0.012, 0.4);
            grain(&mut o, 0.0, 1200.0, 0.002, 1.0, r);
            (o, -23.0)
        }
    };
    finish(mono, peak_db, pan.clamp(-1.0, 1.0))
}

/// Every sound, with a name, for listening to outside the app.
#[cfg(feature = "capture")]
pub fn bank() -> Vec<(&'static str, Cue)> {
    vec![
        ("pick", Cue::Pick),
        ("tick", Cue::Tick),
        ("bloom", Cue::Bloom(2)),
        ("grow", Cue::Grow),
        ("close", Cue::Close),
        ("rustle-faint", Cue::Rustle(0.1)),
        ("rustle-full", Cue::Rustle(1.0)),
        ("lift", Cue::Lift),
        ("land", Cue::Land),
        ("slide", Cue::Slide),
        ("shed", Cue::Shed),
        ("regrow", Cue::Regrow),
        ("knock", Cue::Knock),
    ]
}

/// Writes interleaved stereo samples as a WAV file.
#[cfg(feature = "capture")]
pub fn write_wav(path: &str, samples: &[f32]) {
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    let bytes = samples.len() as u32 * 2;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&(RATE as u32).to_le_bytes());
    wav.extend_from_slice(&(RATE as u32 * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    for s in samples {
        wav.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    let _ = std::fs::write(path, wav);
}

/// A stretch of a file playing in the app's own player during a capture: when it started and
/// stopped on the recording, and where in the file it started.
#[cfg(feature = "capture")]
pub struct Stretch {
    pub from: f32,
    pub to: f32,
    pub path: std::path::PathBuf,
    pub at: f32,
}

/// A capture's soundtrack: exactly what the app played, where it played it. `start` is when the
/// recording began, on the same clock as the log.
#[cfg(feature = "capture")]
pub fn soundtrack(log: &[Logged], music: &[Stretch], start: f32, seconds: f32) -> Vec<f32> {
    use rodio::source::Source;
    let mut track = vec![0.0f32; (seconds * RATE) as usize * 2];
    for played in log {
        let begin = ((played.at - start).max(0.0) * RATE) as usize * 2;
        for (i, s) in render(played.cue, played.seed, played.pan).iter().enumerate() {
            if let Some(slot) = track.get_mut(begin + i) {
                *slot += s * played.gain;
            }
        }
    }
    for stretch in music {
        let Ok(file) = std::fs::File::open(&stretch.path) else { continue };
        let Ok(source) = rodio::Decoder::try_from(file) else { continue };
        let channels = source.channels().get() as usize;
        let rate = source.sample_rate().get() as f32;
        let samples: Vec<f32> = source.collect();
        let begin = ((stretch.from - start).max(0.0) * RATE) as usize;
        let len = ((stretch.to - stretch.from).max(0.0) * RATE) as usize;
        let fade = (0.012 * RATE) as usize;
        for i in 0..len {
            // A plain step through the file at its own pace, resampled by the nearest sample.
            let at = (stretch.at * rate) as usize + (i as f32 * rate / RATE) as usize;
            let Some(frame) = samples.get(at * channels..(at + 1) * channels) else { break };
            let edge = (i.min(len - i) as f32 / fade as f32).min(1.0);
            let (left, right) = (frame[0], frame[channels - 1]);
            if let Some(slot) = track.get_mut((begin + i) * 2) {
                *slot += left * edge;
            }
            if let Some(slot) = track.get_mut((begin + i) * 2 + 1) {
                *slot += right * edge;
            }
        }
    }
    track
}

/// Lays sounds out on one track, at the times given in seconds.
#[cfg(feature = "capture")]
pub fn tour(events: &[(f32, Cue, f32)]) -> Vec<f32> {
    let mut track: Vec<f32> = Vec::new();
    for (i, (at, cue, pan)) in events.iter().enumerate() {
        let sound = render(*cue, 1000 + i as u32 * 77, *pan);
        let start = (*at * RATE) as usize * 2;
        if track.len() < start + sound.len() {
            track.resize(start + sound.len(), 0.0);
        }
        for (j, s) in sound.iter().enumerate() {
            track[start + j] += s;
        }
    }
    track
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Cue; 12] = [Cue::Pick, Cue::Tick, Cue::Bloom(0), Cue::Grow, Cue::Close, Cue::Rustle(0.6), Cue::Lift, Cue::Land, Cue::Slide, Cue::Shed, Cue::Regrow, Cue::Knock];

    #[test]
    fn every_sound_is_quiet_clean_and_ends_in_silence() {
        for cue in ALL {
            for seed in 1..6 {
                let s = render(cue, seed, 0.3);
                assert!(s.iter().all(|x| x.is_finite()), "{cue:?} made something that isn't a number");
                let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
                assert!(peak > 0.03 && peak < 0.2, "{cue:?} peaks at {peak}, which isn't subtle");
                assert!(s[s.len() - 2].abs() < 0.001, "{cue:?} doesn't end in silence");
                assert!(s.len() < 3 * RATE as usize * 2, "{cue:?} is too long");
            }
        }
    }

    /// Needs a real sound device, and makes a real, quiet sound: cargo test -- --ignored
    #[test]
    #[ignore]
    fn a_real_device_opens_quickly_and_plays_every_sound() {
        let started = Instant::now();
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().expect("a sound device");
        sink.log_on_drop(false);
        println!("device opened in {} ms", started.elapsed().as_millis());
        for cue in ALL {
            let samples = render(cue, 7, 0.0);
            let seconds = samples.len() as f32 / 2.0 / RATE;
            let player = rodio::Player::connect_new(sink.mixer());
            player.append(rodio::buffer::SamplesBuffer::new(NonZero::new(2).unwrap(), NonZero::new(RATE as u32).unwrap(), samples));
            let started = Instant::now();
            player.sleep_until_end();
            let took = started.elapsed().as_secs_f32();
            println!("{cue:?}: {seconds:.2} s long, played in {took:.2} s");
            assert!((took - seconds).abs() < 0.15, "{cue:?} didn't play in real time");
        }
    }

    #[test]
    fn no_two_takes_are_the_same() {
        assert_ne!(render(Cue::Bloom(1), 1, 0.0), render(Cue::Bloom(1), 2, 0.0));
    }

    #[test]
    fn a_burst_of_the_same_sound_is_thinned_out() {
        let mut sounds = Sounds::default();
        for i in 0..50 {
            sounds.play(Cue::Pick, i as f32 * 0.001, 0.0, 1.0, 0.0);
        }
        assert_eq!(sounds.heard.len(), 1);
        sounds.play(Cue::Pick, 1.0, 0.0, 1.0, 0.0);
        assert_eq!(sounds.heard.len(), 2);
    }

    #[test]
    fn switched_off_it_makes_no_sound() {
        let mut sounds = Sounds::default();
        sounds.on = false;
        sounds.play(Cue::Grow, 5.0, 0.0, 1.0, 0.0);
        assert!(sounds.heard.is_empty());
    }
}
