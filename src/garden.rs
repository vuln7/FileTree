//! The world around the tree: the time of day, the wind, and the small lives that pass through.
//!
//! All of it is scenery. Nothing here can be clicked, and nothing here is shaped like a flower,
//! because on this tree a flower always means a file. It moves only while someone is here: it
//! eases to a stop when the window loses focus or goes untouched for a while, and with
//! reduced motion on it never starts.

use crate::app::App;
use crate::gpu::FlowerInst;
use crate::palette as p;
use crate::tree::{Kind, NONE, NodeId, rand01};
use crate::ui::{rect, soft_rect, with_alpha};
use std::f32::consts::{PI, TAU};

/// How long the world keeps moving with nobody touching anything, in seconds.
pub const REST_AFTER: f32 = 180.0;
const BUTTERFLIES: usize = 3;
const MAX_FALLING_LEAVES: usize = 6;
const FIREFLIES: u32 = 16;

struct Butterfly {
    pos: [f32; 2],
    vel: [f32; 2],
    target: NodeId,
    /// Somewhere to fly to when there's no flower to visit.
    aim: [f32; 2],
    resting: f32,
    phase: f32,
    colour: f32,
    /// Where on its flower it settles, and which flower that was worked out for.
    perch: [f32; 2],
    perch_on: NodeId,
}

struct Flock {
    x: f32,
    y: f32,
    dir: f32,
    speed: f32,
    count: u32,
}

struct FallingLeaf {
    pos: [f32; 2],
    spin: f32,
    angle: f32,
    sway: f32,
    /// 1 while falling, shrinking to nothing once it reaches the ground.
    size: f32,
}

#[derive(Default)]
pub struct Garden {
    /// How awake the world is, from 0 stopped to 1 fully moving. It eases rather than switches,
    /// so everything slows to a stop together.
    pub level: f32,
    /// World time. It only advances as fast as `level`, so it stands still at rest.
    pub clock: f32,
    butterflies: Vec<Butterfly>,
    flock: Option<Flock>,
    next_flock: f32,
    leaves: Vec<FallingLeaf>,
    next_leaf: f32,
    roll: u32,
    last_selected: NodeId,
    /// The hour of the day, 0 to 24, and when to look at the clock again.
    pub hour: f32,
    next_hour_check: f32,
    pub hour_override: Option<f32>,
    /// Kept still whatever else is going on, because this machine has to draw in software.
    pub held_still: bool,
}

/// A slow, uneven push of air, from about -1 to 1.
fn gust_at(clock: f32) -> f32 {
    (((clock * 0.23).sin() + (clock * 0.61 + 1.3).sin() * 0.6 + (clock * 1.7 + 0.4).sin() * 0.2) / 1.8).clamp(-1.0, 1.0)
}

/// World height of a layer of soil's cut edge: the same curve the shader cuts it with.
fn soil_height(wx: f32, layer: f32) -> f32 {
    let phase = layer * 2.7;
    let wave = (wx * 0.0061 + 1.0 + phase).sin() * 15.0 + (wx * 0.017 + 4.0 - phase).sin() * 6.0 + (wx * 0.043 + phase).sin() * 2.2;
    6.0 + wave * 0.55 - layer * 22.0
}

/// The direction a blade pointing along this screen vector needs as its rotation.
fn along(dx: f32, dy: f32) -> f32 {
    dy.atan2(dx)
}

impl App {
    fn ambient_wanted(&self) -> bool {
        self.focused && !self.reduced_motion && !self.garden.held_still && self.idle_for < REST_AFTER
    }

    /// The breeze's current lean, scaled by how awake the world is.
    pub fn gust(&self) -> f32 {
        gust_at(self.garden.clock) * self.garden.level
    }

    /// Advances the world. True while it's moving.
    pub fn step_garden(&mut self, dt: f32) -> bool {
        self.refresh_hour();
        let wanted = if self.ambient_wanted() { 1.0 } else { 0.0 };
        let garden = &mut self.garden;
        garden.level += (wanted - garden.level) * (1.0 - (-dt * 2.2).exp());
        if (garden.level - wanted).abs() < 0.002 {
            garden.level = wanted;
        }
        if garden.level == 0.0 {
            return false;
        }
        let d = dt * garden.level;
        garden.clock += d;

        self.step_butterflies(d);
        self.step_flock(d);
        self.step_leaves(d);
        true
    }

    fn refresh_hour(&mut self) {
        if let Some(hour) = self.garden.hour_override {
            self.garden.hour = hour;
            return;
        }
        if self.wall >= self.garden.next_hour_check {
            self.garden.next_hour_check = self.wall + 30.0;
            self.garden.hour = crate::platform::local_hour();
        }
    }

    fn roll(&mut self) -> f32 {
        self.garden.roll = self.garden.roll.wrapping_add(1);
        rand01(0x51f2_7a3d, self.garden.roll)
    }

    fn to_world(&self, x: f32, y: f32) -> [f32; 2] {
        let z = self.camera.zoom();
        [(x - self.camera.ox.value) / z, (self.camera.oy.value - y) / z]
    }

    fn to_screen(&self, w: [f32; 2]) -> [f32; 2] {
        let z = self.camera.zoom();
        [self.camera.ox.value + w[0] * z, self.camera.oy.value - w[1] * z]
    }

    /// Somewhere in the open air of the stage.
    fn somewhere(&mut self) -> [f32; 2] {
        let stage = self.stage();
        let (a, b) = (self.roll(), self.roll());
        let x = stage[0] + (stage[2] - stage[0]) * (0.08 + 0.84 * a);
        let y = stage[1] + (stage[3] - stage[1]) * (0.1 + 0.62 * b);
        self.to_world(x, y)
    }

    /// A flower on screen, picked at random.
    fn a_flower(&mut self) -> NodeId {
        if self.visible.is_empty() {
            return NONE;
        }
        let stage = self.stage();
        for _ in 0..10 {
            let at = (self.roll() * self.visible.len() as f32) as usize % self.visible.len();
            let v = &self.visible[at];
            let taken = self.garden.butterflies.iter().any(|b| b.target == v.id);
            if v.rank == 0 && !taken && v.x > stage[0] && v.x < stage[2] && v.y > stage[1] && v.y < stage[3] && v.r > 12.0 * self.scale {
                return v.id;
            }
        }
        NONE
    }

    /// How many butterflies the stage has room for.
    fn butterflies_here(&self) -> usize {
        let room = (self.stage()[2] - self.stage()[0]) / self.scale;
        if room < 620.0 {
            1
        } else if room < 1000.0 {
            2
        } else {
            BUTTERFLIES
        }
    }

    /// The size of small lives on screen: smaller as the tree gets further away, and in a small
    /// window, so they're never out of scale with what they're visiting.
    fn life_size(&self) -> f32 {
        let far = (self.camera.zoom() / self.scale).powf(0.3).clamp(0.6, 1.1);
        let room = ((self.stage()[2] - self.stage()[0]) / (900.0 * self.scale)).clamp(0.75, 1.0);
        far * room
    }

    /// A spot on the rim of a flower, on its upper side, in world units from its centre.
    fn a_perch(&mut self, flower: NodeId) -> [f32; 2] {
        let z = self.camera.zoom().max(0.0001);
        let radius = self.visible.iter().find(|v| v.id == flower).map_or(22.0 * self.scale, |v| v.r);
        let angle = 0.35 + self.roll() * (PI - 0.7);
        let reach = (radius * 0.9 + 3.0 * self.scale) / z;
        [angle.cos() * reach, angle.sin() * reach]
    }

    fn step_butterflies(&mut self, d: f32) {
        let s = self.scale;
        let z = self.camera.zoom().max(0.0001);
        if self.garden.butterflies.is_empty() {
            for i in 0..BUTTERFLIES {
                let from = self.somewhere();
                let aim = self.somewhere();
                self.garden.butterflies.push(Butterfly {
                    pos: from,
                    vel: [0.0, 0.0],
                    target: NONE,
                    aim,
                    resting: 0.0,
                    phase: i as f32 * 2.1,
                    // Colours that stand out on blue, and on most flowers.
                    colour: [7.0, 1.0, 0.0][i % 3],
                    perch: [0.0, 0.0],
                    perch_on: NONE,
                });
            }
        }
        // The first one to notice goes straight to whatever has just been chosen.
        if self.selected != self.garden.last_selected {
            self.garden.last_selected = self.selected;
            if self.tree.is_valid(self.selected) && self.tree.get(self.selected).is_file() {
                let chosen = self.selected;
                let butterfly = &mut self.garden.butterflies[0];
                butterfly.target = chosen;
                butterfly.resting = 0.0;
            }
        }
        let (width, height) = (self.width, self.height);
        for i in 0..self.butterflies_here() {
            let (mut target, resting) = (self.garden.butterflies[i].target, self.garden.butterflies[i].resting);
            // A flower may have left the tree or gone out of reach.
            if target != NONE && (!self.tree.is_valid(target) || !matches!(self.tree.get(target).kind, Kind::File(_)) || self.tree.get(target).grow.target == 0.0) {
                target = NONE;
                self.garden.butterflies[i].target = NONE;
                self.garden.butterflies[i].resting = 0.0;
            }
            if target != NONE && self.garden.butterflies[i].perch_on != target {
                let perch = self.a_perch(target);
                let butterfly = &mut self.garden.butterflies[i];
                butterfly.perch = perch;
                butterfly.perch_on = target;
            }
            let perch = self.garden.butterflies[i].perch;
            let tip = (target != NONE).then(|| {
                let tip = self.tree.get(target).tip;
                [tip[0] + perch[0], tip[1] + perch[1]]
            });

            // Resting on a flower, it rides the flower as it sways.
            if resting > 0.0 {
                let left = resting - d;
                let butterfly = &mut self.garden.butterflies[i];
                butterfly.resting = left;
                butterfly.phase += d * 1.15;
                butterfly.vel = [0.0, 0.0];
                // It rides the flower as it sways.
                if let Some(tip) = tip {
                    butterfly.pos = tip;
                }
                if left <= 0.0 {
                    butterfly.resting = 0.0;
                    butterfly.target = NONE;
                    let next = self.a_flower();
                    let aim = self.somewhere();
                    let butterfly = &mut self.garden.butterflies[i];
                    butterfly.target = next;
                    butterfly.aim = aim;
                }
                continue;
            }

            let goal = tip.unwrap_or(self.garden.butterflies[i].aim);
            let butterfly = &self.garden.butterflies[i];
            let (dx, dy) = (goal[0] - butterfly.pos[0], goal[1] - butterfly.pos[1]);
            let dist = (dx * dx + dy * dy).sqrt().max(0.0001);
            let near = 5.0 * s / z;
            if dist < near {
                if tip.is_some() {
                    let stay = 4.0 + self.roll() * 5.0;
                    self.garden.butterflies[i].resting = stay;
                } else {
                    let next = self.a_flower();
                    let aim = self.somewhere();
                    let butterfly = &mut self.garden.butterflies[i];
                    butterfly.target = next;
                    butterfly.aim = aim;
                }
                continue;
            }
            // It flutters: a wandering sidestep on top of heading where it means to go.
            // It slows as it comes in to land.
            let speed = 78.0 * s / z * (dist / (40.0 * s / z)).clamp(0.3, 1.0);
            let butterfly = &mut self.garden.butterflies[i];
            let flutter = (butterfly.phase * 0.31).sin() * 0.75;
            let (ux, uy) = (dx / dist, dy / dist);
            let want = [(ux - uy * flutter) * speed, (uy + ux * flutter) * speed];
            let turn = (d * 3.0).min(1.0);
            butterfly.vel[0] += (want[0] - butterfly.vel[0]) * turn;
            butterfly.vel[1] += (want[1] - butterfly.vel[1]) * turn;
            butterfly.pos[0] += butterfly.vel[0] * d;
            butterfly.pos[1] += butterfly.vel[1] * d;
            butterfly.phase += d * 13.0;
            // Left far behind by a big camera move, it comes back in from just off screen.
            let pos = butterfly.pos;
            let at = self.to_screen(pos);
            if at[0] < -width || at[0] > width * 2.0 || at[1] < -height || at[1] > height * 2.0 {
                let side = if self.roll() < 0.5 { -30.0 } else { self.stage()[2] + 30.0 };
                let y = height * (0.2 + 0.5 * self.roll());
                let pos = self.to_world(side, y);
                self.garden.butterflies[i].pos = pos;
            }
        }
    }

    fn step_flock(&mut self, d: f32) {
        let clock = self.garden.clock;
        if let Some(flock) = &mut self.garden.flock {
            flock.x += flock.dir * flock.speed * d;
            let gone = if flock.dir > 0.0 { flock.x > self.width + 200.0 * self.scale } else { flock.x < -200.0 * self.scale };
            if gone {
                self.garden.flock = None;
                let wait = 26.0 + self.roll() * 40.0;
                self.garden.next_flock = clock + wait;
            }
        } else if clock >= self.garden.next_flock {
            if self.garden.next_flock == 0.0 {
                // The first flock comes soon after opening, then they're occasional.
                self.garden.next_flock = 7.0;
                return;
            }
            // Birds are asleep at night.
            if self.sky()[2] > 0.4 {
                self.garden.next_flock = clock + 30.0;
                return;
            }
            let s = self.scale;
            let dir = if self.roll() < 0.5 { 1.0 } else { -1.0 };
            let stage = self.stage();
            let y = stage[1] + (stage[3] - stage[1]) * (0.1 + 0.28 * self.roll());
            let speed = (68.0 + 36.0 * self.roll()) * s;
            let count = 3 + (self.roll() * 4.0) as u32;
            let x = if dir > 0.0 { -160.0 * s } else { self.width + 160.0 * s };
            self.garden.flock = Some(Flock { x, y, dir, speed, count });
        }
    }

    fn step_leaves(&mut self, d: f32) {
        let clock = self.garden.clock;
        let gust = gust_at(clock);
        let s = self.scale;
        let z = self.camera.zoom().max(0.0001);
        // A stronger push of air shakes one loose sooner.
        if clock >= self.garden.next_leaf && self.garden.leaves.len() < MAX_FALLING_LEAVES && !self.visible.is_empty() {
            let wait = (3.5 + self.roll() * 6.0) * (1.25 - gust.abs() * 0.6);
            self.garden.next_leaf = clock + wait;
            let stage = self.stage();
            let at = (self.roll() * self.visible.len() as f32) as usize % self.visible.len();
            let v = &self.visible[at];
            if v.rank >= 1 && v.x > stage[0] && v.x < stage[2] && v.y > stage[1] {
                let pos = self.to_world(v.x, v.y);
                let (spin, angle) = (self.roll(), self.roll());
                self.garden.leaves.push(FallingLeaf {
                    pos,
                    spin: (spin - 0.5) * 2.6,
                    angle: angle * TAU,
                    sway: angle * TAU,
                    size: 1.0,
                });
            }
        }
        let fall = 42.0 * s / z;
        let drift = gust * 34.0 * s / z;
        for leaf in &mut self.garden.leaves {
            leaf.sway += d * 1.9;
            leaf.angle += leaf.spin * d + leaf.sway.cos() * d * 0.8;
            leaf.pos[0] += (drift + leaf.sway.sin() * 26.0 * s / z) * d;
            if leaf.pos[1] > soil_height(leaf.pos[0], 0.0) {
                leaf.pos[1] -= fall * d;
            } else {
                // Landed: it settles into the ground and is gone.
                leaf.size -= d * 1.5;
            }
        }
        self.garden.leaves.retain(|leaf| leaf.size > 0.0);
    }

    /// The sun or moon's place, how dark it is, and how warm the horizon is, for the shader.
    pub fn sky(&self) -> [f32; 4] {
        let hour = self.garden.hour;
        // Up at 5:30 and down at 7pm, so at the warmest light it's still sitting on the hills.
        let (day_start, day_end) = (5.5, 19.0);
        let in_day = hour >= day_start && hour < day_end;
        // The moon is up from 8pm to 5am. Either side of that it's down behind the hills, so
        // the sky is empty while the light changes, and nothing rises in the wrong colour.
        let t = if in_day {
            (hour - day_start) / (day_end - day_start)
        } else {
            let since = if hour >= day_end { hour - 20.0 } else { hour + 4.0 };
            (since / 9.0).clamp(0.0, 1.0)
        };
        let x = 0.1 + 0.8 * t;
        let y = 0.98 - 0.82 * (t * PI).sin().max(0.0).powf(0.6);
        let smooth = |a: f32, b: f32, v: f32| {
            let k = ((v - a) / (b - a)).clamp(0.0, 1.0);
            k * k * (3.0 - 2.0 * k)
        };
        let night = smooth(19.0, 20.5, hour).max(1.0 - smooth(4.5, 6.0, hour));
        let warm = |centre: f32| (1.0 - ((hour - centre).abs() / 1.6)).clamp(0.0, 1.0);
        let glow = warm(6.0).max(warm(18.5));
        [x, y, night, glow]
    }

    /// Grass along both cuts of the soil. Only leaves, never flowers.
    pub fn draw_grass(&mut self) {
        let z = self.camera.zoom();
        let s = self.scale;
        if 6.0 * z < 2.2 * s {
            return;
        }
        let (ox, oy) = (self.camera.ox.value, self.camera.oy.value);
        if oy + 40.0 * z < self.toolbar_height() || oy - 60.0 * z > self.height + 40.0 * s {
            return;
        }
        let stage = self.stage();
        // Spaced in world units, so the grass stays put; thinned by powers of two when small.
        let mut step = 15.0f32;
        while (stage[2] - stage[0]) / (step * z) > 110.0 {
            step *= 2.0;
        }
        let first = ((stage[0] - ox) / z / step).floor() as i32 - 1;
        let last = ((stage[2] - ox) / z / step).ceil() as i32 + 1;
        let clock = self.garden.clock;
        let gust = self.gust();
        let level = self.garden.level;
        for cell in first..=last {
            let seed = (cell as u32).wrapping_mul(2_654_435_761);
            for (row, layer) in [(0u32, 0.0f32), (1, 1.0)] {
                for blade in 0..2u32 {
                    let channel = row * 10 + blade;
                    let wx = (cell as f32 + rand01(seed, channel + 81)) * step;
                    // It grows in tufts, with bare ground between them.
                    let tuft = (wx * 0.0131 + row as f32 * 2.0).sin() + (wx * 0.0373 + 1.7).sin() * 0.7;
                    if tuft < 0.15 || rand01(seed, channel + 80) < 0.25 {
                        continue;
                    }
                    let radius = ((3.0 + rand01(seed, channel + 82) * 3.6) * z).min(8.0 * s);
                    let lean = (rand01(seed, channel + 83) - 0.5) * 1.1;
                    let sway = (clock * 1.25 + wx * 0.021 + blade as f32).sin() * 0.11 * level + gust * 0.2;
                    let phi = lean + sway;
                    let (dx, dy) = (phi.sin(), -phi.cos());
                    let root = [ox + wx * z, oy - soil_height(wx, layer) * z + 2.0 * s];
                    let inst = FlowerInst {
                        pos: [root[0] + dx * radius * 0.92, root[1] + dy * radius * 0.92],
                        radius,
                        rotation: along(dx, dy),
                        bloom: 1.0,
                        species: p::LEAF,
                        seed: rand01(seed, channel + 84) * 40.0,
                        lift: 0.0,
                        dim: if row == 0 { 0.3 } else { 0.0 },
                    };
                    if row == 0 {
                        self.draw.backdrop.push(inst);
                    } else {
                        self.draw.leaves.push(inst);
                    }
                }
            }
        }
    }

    /// Whatever is flying or falling, in front of the tree and behind its names.
    pub fn draw_garden(&mut self) {
        if self.reduced_motion {
            return;
        }
        let s = self.scale;
        let clock = self.garden.clock;

        if let Some(flock) = &self.garden.flock {
            for i in 0..flock.count {
                let rank = i.div_ceil(2) as f32;
                let side = if i % 2 == 0 { 1.0 } else { -1.0 };
                let i = i as f32;
                let x = flock.x - flock.dir * rank * 30.0 * s;
                let y = flock.y + side * rank * 15.0 * s + (clock * 0.7 + i).sin() * 3.0 * s;
                // A few beats, then a long glide on held wings, each bird in its own time.
                let beating = ((clock * 0.8 + i * 1.3).sin() * 1.6 + 0.3).clamp(0.0, 1.0);
                let beat = 0.5 + 0.5 * (clock * 9.0 + i * 1.7).sin();
                self.draw.backdrop.push(FlowerInst {
                    pos: [x, y],
                    radius: (11.0 - rank * 0.8).max(8.0) * s,
                    rotation: flock.dir * 0.05,
                    bloom: 0.62 + (beat - 0.62) * beating,
                    species: p::BIRD,
                    seed: 0.0,
                    lift: 0.0,
                    dim: 0.0,
                });
            }
        }

        let z = self.camera.zoom();
        let (ox, oy) = (self.camera.ox.value, self.camera.oy.value);
        for leaf in &self.garden.leaves {
            self.draw.top_flowers.push(FlowerInst {
                pos: [ox + leaf.pos[0] * z, oy - leaf.pos[1] * z],
                radius: 8.5 * s * leaf.size.clamp(0.0, 1.0),
                rotation: leaf.angle,
                bloom: 1.0,
                species: p::LEAF,
                seed: 7.0,
                lift: 0.45,
                dim: 0.0,
            });
        }

        let size = self.life_size();
        for i in 0..self.butterflies_here().min(self.garden.butterflies.len()) {
            let butterfly = &self.garden.butterflies[i];
            let resting = butterfly.resting > 0.0;
            let wave = butterfly.phase.sin().abs();
            let (heading, open, bob, lift) = if resting {
                // Facing the heart of the flower, wings opening and closing slowly.
                let perch = butterfly.perch;
                (along(-perch[0], perch[1]), 0.3 + 0.62 * wave, 0.0, 0.1)
            } else {
                // The world's y runs up and the screen's runs down. It rises on each beat.
                (along(butterfly.vel[0], -butterfly.vel[1]), 0.2 + 0.8 * wave, wave * 2.4 * s, 0.55)
            };
            self.draw.top_flowers.push(FlowerInst {
                pos: [ox + butterfly.pos[0] * z, oy - butterfly.pos[1] * z - bob],
                radius: 13.0 * s * size,
                rotation: heading,
                bloom: open,
                species: p::BUTTERFLY,
                seed: butterfly.colour,
                lift,
                dim: 0.0,
            });
        }
    }

    /// Fireflies, low over the ground on a dark night.
    pub fn draw_fireflies(&mut self) {
        let night = self.sky()[2];
        if night < 0.05 || self.reduced_motion {
            return;
        }
        let s = self.scale;
        let clock = self.garden.clock;
        let stage = self.stage();
        let z = self.camera.zoom();
        let ground = (self.camera.oy.value - 6.0 * z).min(stage[3]);
        let colour = crate::gpu::rgb(p::FIREFLY);
        let l = self.layer(0);
        for i in 0..FIREFLIES {
            let (a, b, c) = (rand01(0x6d2b_79f5, i), rand01(0x6d2b_79f5, i + 100), rand01(0x6d2b_79f5, i + 200));
            let x = stage[0] + (stage[2] - stage[0]) * a + (clock * (0.17 + c * 0.2) + i as f32).sin() * 46.0 * s;
            let y = ground - (24.0 + b * 210.0) * s + (clock * (0.23 + c * 0.2) + c * TAU).cos() * 24.0 * s;
            if y < stage[1] {
                continue;
            }
            let pulse = ((clock * (0.55 + c * 0.7) + i as f32 * 2.3).sin()).max(0.0);
            let glow = pulse * pulse * night;
            if glow < 0.02 {
                continue;
            }
            let (halo, core) = (7.0 * s, 1.6 * s);
            l.rects.push(soft_rect([x - halo, y - halo], [x + halo, y + halo], halo, 5.0 * s, with_alpha(colour, 0.34 * glow)));
            l.rects.push(rect([x - core, y - core], [x + core, y + core], core, with_alpha(colour, glow)));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::fs::Waker;
    use std::sync::Arc;

    fn app() -> App {
        let waker: Waker = Arc::new(|| {});
        let mut app = App::new(None, waker, 1440.0, 900.0, 1.0);
        app.garden.hour_override = Some(12.0);
        app
    }

    fn run(app: &mut App, seconds: f32) {
        for _ in 0..(seconds * 60.0) as usize {
            app.tick(1.0 / 60.0);
        }
    }

    #[test]
    fn the_world_moves_while_someone_is_here() {
        let mut app = app();
        run(&mut app, 3.0);
        assert!(app.garden.level > 0.9);
        let before = app.garden.clock;
        assert!(app.tick(1.0 / 60.0), "it keeps asking for frames");
        assert!(app.garden.clock > before);
    }

    #[test]
    fn it_comes_to_a_complete_stop_when_the_window_loses_focus() {
        let mut app = app();
        run(&mut app, 2.0);
        app.set_focused(false);
        run(&mut app, 6.0);
        assert_eq!(app.garden.level, 0.0);
        assert!(!app.tick(1.0 / 60.0), "nothing should ask for another frame");
    }

    #[test]
    fn it_comes_to_a_complete_stop_after_being_left_alone() {
        let mut app = app();
        run(&mut app, 1.0);
        app.idle_for = super::REST_AFTER + 1.0;
        run(&mut app, 6.0);
        assert!(!app.tick(1.0 / 60.0));
        app.wake_wind();
        run(&mut app, 0.5);
        assert!(app.garden.level > 0.1, "and a touch wakes it again");
    }

    #[test]
    fn reduced_motion_keeps_it_still() {
        let mut app = app();
        app.reduced_motion = true;
        app.garden.level = 0.0;
        run(&mut app, 2.0);
        assert_eq!(app.garden.clock, 0.0);
    }

    #[test]
    fn selecting_with_the_panel_away_draws_without_trouble() {
        let mut app = app();
        run(&mut app, 0.5);
        app.toggle_panel();
        for _ in 0..120 {
            app.tick(1.0 / 60.0);
            app.text.begin_frame();
            app.build_scene(1.0 / 60.0);
            app.build_ui();
        }
        // The panel's contents would be easing in now, into a layer with nothing in it.
        app.acts.panel_in = 0.0;
        app.text.begin_frame();
        app.build_ui();
    }

    #[test]
    fn drawing_in_software_keeps_it_still() {
        let mut app = app();
        app.garden.held_still = true;
        run(&mut app, 2.0);
        assert_eq!(app.garden.clock, 0.0);
    }

    #[test]
    fn birds_pass_by_day_and_sleep_at_night() {
        let mut day = app();
        run(&mut day, 10.0);
        assert!(day.garden.flock.is_some(), "a flock should be crossing by now");
        let mut night = app();
        night.garden.hour_override = Some(23.0);
        run(&mut night, 10.0);
        assert!(night.garden.flock.is_none());
    }

    #[test]
    fn a_small_window_has_room_for_fewer_butterflies() {
        let waker: Waker = Arc::new(|| {});
        let small = App::new(None, waker, 700.0, 460.0, 1.0);
        assert!(small.butterflies_here() < app().butterflies_here());
    }

    #[test]
    fn the_sky_knows_day_from_night() {
        let mut app = app();
        app.garden.hour = 12.0;
        let noon = app.sky();
        assert_eq!(noon[2], 0.0, "not dark at noon");
        assert!(noon[1] < 0.3, "the sun is high");
        app.garden.hour = 23.0;
        assert_eq!(app.sky()[2], 1.0, "dark at night");
        app.garden.hour = 19.5;
        assert!(app.sky()[1] > 0.95, "nothing has risen in the half hour after sunset");
        app.garden.hour = 5.2;
        assert!(app.sky()[1] > 0.95, "nor just before dawn");
        app.garden.hour = 18.5;
        assert!(app.sky()[3] > 0.9, "the horizon is warm at sunset");
        assert!(app.sky()[1] > 0.65, "and the sun is low, down by the hills");
    }
}
