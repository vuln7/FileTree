// Everything on screen is a piece of cut paper. All of it is drawn analytically, so
// edges stay crisp at any zoom and no textures are needed apart from text and previews.

struct Globals {
    viewport: vec2f,
    time: f32,
    scale: f32,
    world_origin: vec2f,
    zoom: f32,
    wind: f32,
    ground: vec4f,
    ground_deep: vec4f,
    soil: vec4f,
    shadow: vec4f,
    // Outer petal, inner petal, and centre colours for each species.
    species: array<vec4f, 39>,
}

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

fn to_clip(p: vec2f) -> vec4f {
    return vec4f(p.x / g.viewport.x * 2.0 - 1.0, 1.0 - p.y / g.viewport.y * 2.0, 0.0, 1.0);
}

fn hash21(p: vec2f) -> f32 {
    let q = fract(p * vec2f(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

fn value_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash21(i), hash21(i + vec2f(1.0, 0.0)), u.x),
               mix(hash21(i + vec2f(0.0, 1.0)), hash21(i + vec2f(1.0, 1.0)), u.x), u.y);
}

// The slight tooth of painted paper: fine fibre plus broad brush variation.
fn paper(col: vec3f, p: vec2f) -> vec3f {
    let fibre = hash21(floor(p / g.scale)) - 0.5;
    return col * (1.0 + fibre * 0.035);
}

// ---------------------------------------------------------------- ground

struct GroundOut {
    @builtin(position) pos: vec4f,
}

@vertex
fn vs_ground(@builtin(vertex_index) vi: u32) -> GroundOut {
    var out: GroundOut;
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    out.pos = vec4f(x, y, 0.0, 1.0);
    return out;
}

@fragment
fn fs_ground(in: GroundOut) -> @location(0) vec4f {
    let p = in.pos.xy;
    // Broad horizontal brush strokes in the gouache.
    let streak = value_noise(p * vec2f(0.0026, 0.026) / g.scale);
    let streak2 = value_noise(p * vec2f(0.009, 0.09) / g.scale + 7.3);
    let stroke = smoothstep(0.32, 0.68, streak * 0.62 + streak2 * 0.38);
    var col = mix(g.ground.rgb, g.ground_deep.rgb, stroke * 0.42);

    // The back layer of soil, cut with a wandering edge, its shadow falling on the field.
    let d = p.y - soil_edge(p.x, 0.0);
    let drop = clamp(1.0 + d / (9.0 * g.scale), 0.0, 1.0);
    col = mix(col, g.shadow.rgb, drop * drop * 0.42 * step(d, 0.0));
    let soil = clamp(d + 0.5, 0.0, 1.0);
    let soil_streak = value_noise(p * vec2f(0.01, 0.04) / g.scale + 19.0);
    col = mix(col, g.soil.rgb * (0.82 + soil_streak * 0.07), soil);
    return vec4f(paper(col, p), 1.0);
}

fn soil_edge(x: f32, layer: f32) -> f32 {
    let wx = (x - g.world_origin.x) / max(g.zoom, 0.0001);
    let phase = layer * 2.7;
    let wave = sin(wx * 0.0061 + 1.0 + phase) * 15.0 + sin(wx * 0.017 + 4.0 - phase) * 6.0 + sin(wx * 0.043 + phase) * 2.2;
    // World height of the cut edge, measured up from the world origin.
    let height = 6.0 + wave * 0.55 - layer * 22.0;
    return g.world_origin.y - height * g.zoom;
}

// The front layer of soil, over the foot of the trunk.
@fragment
fn fs_soil_front(in: GroundOut) -> @location(0) vec4f {
    let p = in.pos.xy;
    let d = p.y - soil_edge(p.x, 1.0);
    if (d < -12.0 * g.scale) { discard; }
    let cover = clamp(d + 0.5, 0.0, 1.0);
    let drop = clamp(1.0 + d / (8.0 * g.scale), 0.0, 1.0);
    let shadow_a = drop * drop * 0.38 * (1.0 - cover);
    let streak = value_noise(p * vec2f(0.012, 0.05) / g.scale + 31.0);
    let col = paper(g.soil.rgb * (1.04 + streak * 0.06), p);
    let rgb = col * cover + g.shadow.rgb * shadow_a;
    return vec4f(rgb, cover + shadow_a);
}

// ---------------------------------------------------------------- branches

struct BranchIn {
    @location(0) p0: vec2f,
    @location(1) c1: vec2f,
    @location(2) c2: vec2f,
    @location(3) p1: vec2f,
    // start width, end width, grow, seed
    @location(4) w: vec4f,
    // segments, shadow lift, dim, unused
    @location(5) misc: vec4f,
    @location(6) color: vec4f,
}

struct BranchOut {
    @builtin(position) pos: vec4f,
    @location(0) edge: f32,
    @location(1) half_width: f32,
    @location(2) color: vec4f,
    @location(3) screen: vec2f,
    @location(4) shade: f32,
}

const CAP_ROWS: f32 = 4.0;

fn branch_vertex(vi: u32, b: BranchIn, grow_offset: vec2f, feather: f32) -> BranchOut {
    let segs = b.misc.x;
    let row = f32(vi / 2u);
    let side = f32(vi & 1u) * 2.0 - 1.0;
    let grow = b.w.z;
    let along = min(row / segs, 1.0);
    // Rows past the end of the curve round off the tip, so a limb ends as a cut curve.
    let cap = max(row - segs, 0.0) / CAP_ROWS;
    let t = along;
    let u = 1.0 - t;
    let p = u * u * u * b.p0 + 3.0 * u * u * t * b.c1 + 3.0 * u * t * t * b.c2 + t * t * t * b.p1;
    var d = 3.0 * u * u * (b.c1 - b.p0) + 6.0 * u * t * (b.c2 - b.c1) + 3.0 * t * t * (b.p1 - b.c2);
    let l = length(d);
    if (l < 0.0001) { d = vec2f(0.0, -1.0); } else { d = d / l; }
    let normal = vec2f(-d.y, d.x);
    // Scissor cuts: each edge is a run of short straight facets, cut independently, and
    // they die away at the ends so a limb always meets its neighbours exactly.
    let cut = hash21(vec2f(row * 3.1, b.w.w * 91.0 + side * 7.0)) - 0.5;
    let wobble = 1.0 + cut * 0.2 * sin(t * 3.14159);
    // While growing, the leading end narrows to a point as if being pulled out from behind.
    let lead = 1.0 - pow(along, 8.0) * (1.0 - grow * grow);
    let width = mix(b.w.x, b.w.y, t) * wobble * lead;
    let full_half = max(width * 0.5, 0.4);
    let half_w = max(full_half * sqrt(max(1.0 - cap * cap, 0.0)), 0.01);
    let reach = half_w + feather + 1.0;
    var out: BranchOut;
    let pos = p + d * full_half * cap + normal * side * reach + grow_offset;
    out.pos = to_clip(pos);
    out.edge = side * reach;
    out.half_width = half_w;
    out.color = b.color;
    out.screen = pos;
    out.shade = smoothstep(0.0, 0.3, along);
    return out;
}

@vertex
fn vs_branch(@builtin(vertex_index) vi: u32, b: BranchIn) -> BranchOut {
    return branch_vertex(vi, b, vec2f(0.0), 0.0);
}

@fragment
fn fs_branch(in: BranchOut) -> @location(0) vec4f {
    let cover = clamp(in.half_width - abs(in.edge) + 0.5, 0.0, 1.0);
    let col = paper(in.color.rgb, in.screen);
    return vec4f(col * cover, cover);
}

@vertex
fn vs_branch_shadow(@builtin(vertex_index) vi: u32, b: BranchIn) -> BranchOut {
    let s = g.scale;
    let lift = 1.0 + b.misc.y * 1.8;
    return branch_vertex(vi, b, vec2f(1.6 * s, 3.2 * s) * lift, 4.0 * s);
}

@fragment
fn fs_branch_shadow(in: BranchOut) -> @location(0) vec4f {
    let soft = 3.0 * g.scale;
    let a = smoothstep(in.half_width + soft, in.half_width - soft * 0.5, abs(in.edge)) * 0.36 * in.shade;
    return vec4f(g.shadow.rgb * a, a);
}

// ---------------------------------------------------------------- flowers, leaves, buds

struct FlowerIn {
    @location(0) pos: vec2f,
    @location(1) radius: f32,
    @location(2) rotation: f32,
    @location(3) bloom: f32,
    @location(4) species: f32,
    @location(5) seed: f32,
    @location(6) lift: f32,
    @location(7) dim: f32,
}

struct FlowerOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) radius: f32,
    @location(2) rotation: f32,
    @location(3) bloom: f32,
    @location(4) @interpolate(flat) species: u32,
    @location(5) seed: f32,
    @location(6) lift: f32,
    @location(7) dim: f32,
    @location(8) screen: vec2f,
}

@vertex
fn vs_flower(@builtin(vertex_index) vi: u32, f: FlowerIn) -> FlowerOut {
    var corners = array<vec2f, 4>(vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(-1.0, 1.0), vec2f(1.0, 1.0));
    let c = corners[vi];
    // Enough room for petals plus the shadow they throw.
    let reach = f.radius * 1.3 + (6.0 + f.lift * 12.0) * g.scale;
    let p = f.pos + c * reach;
    var out: FlowerOut;
    out.pos = to_clip(p);
    out.uv = c * reach / max(f.radius, 0.01);
    out.radius = f.radius;
    out.rotation = f.rotation;
    out.bloom = f.bloom;
    out.species = u32(f.species);
    out.seed = f.seed;
    out.lift = f.lift;
    out.dim = f.dim;
    out.screen = p;
    return out;
}

struct Shape {
    petals: f32,    // how many around the flower
    width: f32,     // petal half-width relative to length
    point: f32,     // 0 = round tip, 1 = pointed
    inner: f32,     // size of the inner ring, 0 for none
    core: f32,      // size of the centre
    ruffle: f32,    // edge irregularity
}

fn species_shape(s: u32) -> Shape {
    switch s {
        case 0u: { return Shape(13.0, 0.115, 0.15, 0.0, 0.27, 0.012); }  // daisy, documents
        case 1u: { return Shape(5.0, 0.46, 0.0, 0.68, 0.17, 0.045); }    // poppy, images
        case 2u: { return Shape(3.0, 0.66, 0.35, 0.6, 0.2, 0.02); }      // trillium, video
        case 3u: { return Shape(6.0, 0.25, 0.0, 0.6, 0.2, 0.012); }      // bellflower, audio
        case 4u: { return Shape(5.0, 0.16, 1.0, 0.0, 0.25, 0.0); }       // star, code
        case 5u: { return Shape(6.0, 0.36, 0.0, 0.0, 0.2, 0.02); }       // quince, data
        case 6u: { return Shape(17.0, 0.13, 0.0, 0.58, 0.22, 0.0); }     // allium, archives
        case 7u: { return Shape(8.0, 0.21, 0.55, 0.62, 0.17, 0.0); }     // dahlia, apps
        case 12u: { return Shape(6.0, 0.4, 0.0, 0.58, 0.2, 0.03); }      // blossom, a whole disk
        default: { return Shape(4.0, 0.42, 0.1, 0.0, 0.19, 0.03); }      // clover, everything else
    }
}

const TAU: f32 = 6.2831853;

fn rot2(p: vec2f, a: f32) -> vec2f {
    let s = sin(a);
    let c = cos(a);
    return vec2f(c * p.x + s * p.y, -s * p.x + c * p.y);
}

// Signed distance-like value for one petal lying along +x, with root at the origin.
// Positive inside. `len` is the petal length in flower radii.
fn petal(p: vec2f, len: f32, width: f32, point: f32, ruffle: f32, seed: f32) -> f32 {
    if (len < 0.001) { return -1.0; }
    let x = p.x / len;
    if (x <= 0.0 || x >= 1.0) {
        return -max(max(-x, x - 1.0) * len, abs(p.y));
    }
    // A petal is fuller toward its tip; a pointed one is a lens.
    let round = sqrt(max(x * (1.0 - x) * 4.0, 0.0)) * (0.62 + 0.55 * x);
    let lens = pow(sin(x * 3.14159), 1.25);
    let profile = mix(round, lens, point);
    let hand = sin(x * 9.0 + seed) * ruffle + sin(x * 23.0 + seed * 3.0) * ruffle * 0.4;
    return (profile + hand) * width * len - abs(p.y);
}

fn stagger(bloom: f32, i: f32, n: f32) -> f32 {
    // Petals unfold one after another around the flower.
    let span = 0.55;
    let start = (i / n) * span;
    let k = clamp((bloom - start) / (1.0 - span), 0.0, 1.0);
    // Overshoot a little, like paper springing open.
    return 1.0 - pow(1.0 - k, 3.0) * cos(k * 3.14159 * 1.1);
}

// Coverage of one ring of petals, in flower radii units.
fn ring(uv: vec2f, sh: Shape, len: f32, turn: f32, bloom: f32, seed: f32) -> f32 {
    let step_angle = TAU / sh.petals;
    let a = atan2(uv.y, uv.x) - turn;
    let k = floor(a / step_angle + 0.5);
    var best = -10.0;
    // A petal can spill into its neighbours' sectors, so check those too.
    for (var j = -1.0; j <= 1.0; j += 1.0) {
        let raw = k + j;
        let idx = raw - sh.petals * floor(raw / sh.petals);
        let open = stagger(bloom, idx, sh.petals);
        let swing = (1.0 - min(open, 1.0)) * 0.9;
        let q = rot2(uv, raw * step_angle + turn + swing);
        let wide = sh.width * (0.35 + 0.65 * min(open, 1.0));
        best = max(best, petal(q, len * max(open, 0.0), wide, sh.point, sh.ruffle, seed + idx * 2.4));
    }
    return best;
}

fn flower_alpha(uv: vec2f, sh: Shape, rotation: f32, bloom: f32, seed: f32, soft: f32) -> f32 {
    let d = ring(uv, sh, 1.0, rotation, bloom, seed);
    return clamp(d / soft + 0.5, 0.0, 1.0);
}

fn dimmed(col: vec3f, d: f32) -> vec3f {
    return mix(col, col * 0.6 + g.ground.rgb * 0.14, d);
}

@fragment
fn fs_flower(in: FlowerOut) -> @location(0) vec4f {
    let px = 1.0 / max(in.radius, 0.01);
    let s = in.species;
    // A shed petal borrows the colour of the flower it fell from, given in its seed.
    let palette = select(s, u32(in.seed) % 9u, s == 11u);
    let c_outer = g.species[palette * 3u].rgb;
    let c_inner = g.species[palette * 3u + 1u].rgb;
    let c_core = g.species[palette * 3u + 2u].rgb;

    // Shadow offset is in screen pixels, converted to flower radii.
    let lift = in.lift;
    let shadow_shift = vec2f(1.3 + lift * 3.0, 2.4 + lift * 6.0) * g.scale * px;
    let shadow_soft = (1.8 + lift * 3.0) * g.scale * px;

    // Far away a flower is a dot of its colour: cheap, and still legible as a mass.
    if (in.radius < 5.0 * g.scale && (s < 9u || s == 12u)) {
        let r = length(in.uv);
        let size = 0.3 + 0.62 * in.bloom;
        let a = clamp((size - r) / px + 0.5, 0.0, 1.0);
        let col = dimmed(c_outer, in.dim);
        return vec4f(col * a, a);
    }

    var col: vec3f;
    var alpha = 0.0;

    if (s == 11u) {
        let q = rot2(in.uv, in.rotation);
        let qs = rot2(in.uv - shadow_shift, in.rotation);
        let cover = clamp(petal(q + vec2f(1.0, 0.0), 2.0, 0.42, 0.0, 0.03, 3.0) / px + 0.5, 0.0, 1.0);
        let shadow_a = clamp(petal(qs + vec2f(1.0, 0.0), 2.0, 0.42, 0.0, 0.03, 3.0) / shadow_soft + 0.5, 0.0, 1.0) * 0.35;
        return vec4f(paper(c_outer, in.screen), 1.0) * cover + vec4f(g.shadow.rgb, 1.0) * shadow_a * (1.0 - cover);
    }

    if (s == 9u) {
        // A leaf: one pointed blade with a pale rib.
        let q = rot2(in.uv, in.rotation);
        let qs = rot2(in.uv - shadow_shift, in.rotation);
        let blade = petal(q + vec2f(1.0, 0.0), 2.0 * in.bloom, 0.2, 1.0, 0.0, in.seed);
        let shadow_d = petal(qs + vec2f(1.0, 0.0), 2.0 * in.bloom, 0.2, 1.0, 0.0, in.seed);
        let shadow_a = clamp(shadow_d / shadow_soft + 0.5, 0.0, 1.0) * 0.42;
        let cover = clamp(blade / px + 0.5, 0.0, 1.0);
        let rib = clamp((0.022 - abs(q.y)) / px + 0.5, 0.0, 1.0) * step(-0.75, q.x) * step(q.x, 0.8);
        col = mix(c_outer, c_inner, rib * 0.7);
        col = paper(dimmed(col, in.dim), in.screen);
        let out = vec4f(col, 1.0) * cover + vec4f(g.shadow.rgb, 1.0) * shadow_a * (1.0 - cover);
        return out;
    }

    if (s == 10u) {
        // A closed bud: a plump teardrop wrapped by two sepals.
        let q = rot2(in.uv, in.rotation);
        let qs = rot2(in.uv - shadow_shift, in.rotation);
        let body = petal(q + vec2f(0.95, 0.0), 1.95, 0.4, 0.45, 0.0, in.seed);
        let body_s = petal(qs + vec2f(0.95, 0.0), 1.95, 0.4, 0.45, 0.0, in.seed);
        let sep1 = petal(rot2(q + vec2f(0.95, 0.0), 0.3), 1.05, 0.22, 1.0, 0.0, in.seed);
        let sep2 = petal(rot2(q + vec2f(0.95, 0.0), -0.3), 1.05, 0.22, 1.0, 0.0, in.seed);
        let shadow_a = clamp(body_s / shadow_soft + 0.5, 0.0, 1.0) * 0.42;
        let cover = clamp(max(body, max(sep1, sep2)) / px + 0.5, 0.0, 1.0);
        let sepal = clamp(max(sep1, sep2) / px + 0.5, 0.0, 1.0);
        col = mix(c_outer, c_inner, sepal);
        col = paper(dimmed(col, in.dim), in.screen);
        return vec4f(col, 1.0) * cover + vec4f(g.shadow.rgb, 1.0) * shadow_a * (1.0 - cover);
    }

    let sh = species_shape(s);
    let turn = in.rotation + (1.0 - in.bloom) * 1.1;

    // The whole flower's shadow on the page.
    let shadow_a = flower_alpha(in.uv - shadow_shift, sh, turn, in.bloom, in.seed, shadow_soft) * 0.44;

    // Outer ring.
    let outer_d = ring(in.uv, sh, 1.0, turn, in.bloom, in.seed);
    let outer_a = clamp(outer_d / px + 0.5, 0.0, 1.0);

    col = c_outer;
    alpha = outer_a;

    // Inner ring, a second sheet of paper on top, throwing its own small shadow.
    if (sh.inner > 0.0) {
        let inner_bloom = clamp(in.bloom * 1.25 - 0.25, 0.0, 1.0);
        let inner_turn = turn + 3.14159 / sh.petals;
        let inner_shadow = ring(in.uv - shadow_shift * 0.5, sh, sh.inner, inner_turn, inner_bloom, in.seed + 3.0);
        let shade = clamp(inner_shadow / (shadow_soft * 0.7) + 0.5, 0.0, 1.0);
        col = mix(col, col * 0.72, shade * 0.75);
        let inner_d = ring(in.uv, sh, sh.inner, inner_turn, inner_bloom, in.seed + 3.0);
        let inner_a = clamp(inner_d / px + 0.5, 0.0, 1.0);
        col = mix(col, c_inner, inner_a);
    }

    // The heart of the flower: last to appear, and it casts a small shadow too.
    let core_size = sh.core * smoothstep(0.1, 0.7, in.bloom);
    let r = length(in.uv);
    let core_shadow = clamp((core_size - length(in.uv - shadow_shift * 0.35)) / (shadow_soft * 0.5) + 0.5, 0.0, 1.0);
    col = mix(col, col * 0.75, core_shadow * 0.6 * step(0.001, core_size));
    // Cores wobble slightly so they never read as perfect circles.
    let a2 = atan2(in.uv.y, in.uv.x);
    let wobble = sin(a2 * 5.0 + in.seed) * 0.018 + sin(a2 * 3.0 - in.seed * 2.0) * 0.012;
    let core_a = clamp((core_size + wobble * step(0.001, core_size) - r) / px + 0.5, 0.0, 1.0);
    col = mix(col, c_core, core_a);
    alpha = max(alpha, core_a);

    // Before opening, a flower is a tight little bud of its own colour.
    let closed = 1.0 - smoothstep(0.0, 0.22, in.bloom);
    let bud_a = clamp((0.2 * closed - r) / px + 0.5, 0.0, 1.0) * step(0.001, closed);
    col = mix(col, c_outer, bud_a * (1.0 - alpha));
    alpha = max(alpha, bud_a);

    col = paper(dimmed(col, in.dim), in.screen);
    let shadow_alpha = shadow_a * (1.0 - in.dim * 0.7);
    let rgb = col * alpha + g.shadow.rgb * shadow_alpha * (1.0 - alpha);
    let a = alpha + shadow_alpha * (1.0 - alpha);
    return vec4f(rgb, a);
}

// ---------------------------------------------------------------- interface shapes

struct RectIn {
    @location(0) a: vec2f,
    @location(1) b: vec2f,
    // radius, blur, mode, thickness
    @location(2) params: vec4f,
    @location(3) color: vec4f,
}

struct RectOut {
    @builtin(position) pos: vec4f,
    @location(0) p: vec2f,
    @location(1) a: vec2f,
    @location(2) b: vec2f,
    @location(3) params: vec4f,
    @location(4) color: vec4f,
}

@vertex
fn vs_rect(@builtin(vertex_index) vi: u32, r: RectIn) -> RectOut {
    var corners = array<vec2f, 4>(vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0), vec2f(1.0, 1.0));
    let c = corners[vi];
    let mode = r.params.z;
    let grow = r.params.y * 2.0 + 2.0 + select(0.0, r.params.w * 2.0, mode < 0.5);
    var lo: vec2f;
    var hi: vec2f;
    if (mode < 0.5) {
        lo = r.a;
        hi = r.b;
    } else if (mode < 1.5) {
        lo = min(r.a, r.b) - vec2f(r.params.x);
        hi = max(r.a, r.b) + vec2f(r.params.x);
    } else {
        lo = r.a - vec2f(r.b.x + r.params.w);
        hi = r.a + vec2f(r.b.x + r.params.w);
    }
    let p = mix(lo - vec2f(grow), hi + vec2f(grow), c);
    var out: RectOut;
    out.pos = to_clip(p);
    out.p = p;
    out.a = r.a;
    out.b = r.b;
    out.params = r.params;
    out.color = r.color;
    return out;
}

@fragment
fn fs_rect(in: RectOut) -> @location(0) vec4f {
    let mode = in.params.z;
    var d: f32;
    if (mode < 0.5) {
        let center = (in.a + in.b) * 0.5;
        let half_size = (in.b - in.a) * 0.5;
        let q = abs(in.p - center) - half_size + vec2f(in.params.x);
        d = length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - in.params.x;
        if (in.params.w > 0.0) {
            let along = (in.p.x + in.p.y) / g.scale;
            d += in.params.w * (sin(along * 0.023 + 1.3) * 0.6 + sin(along * 0.071) * 0.4);
        }
    } else if (mode < 1.5) {
        let pa = in.p - in.a;
        let ba = in.b - in.a;
        let h = clamp(dot(pa, ba) / max(dot(ba, ba), 0.0001), 0.0, 1.0);
        d = length(pa - ba * h) - in.params.x;
    } else {
        d = abs(length(in.p - in.a) - in.b.x) - in.params.w;
    }
    let blur = max(in.params.y, 0.0);
    var a: f32;
    if (blur > 0.0) {
        a = smoothstep(blur, -blur, d);
    } else {
        a = clamp(0.5 - d, 0.0, 1.0);
    }
    return vec4f(in.color.rgb * in.color.a * a, in.color.a * a);
}

// ---------------------------------------------------------------- text

struct GlyphIn {
    @location(0) pos: vec2f,
    @location(1) size: vec2f,
    @location(2) uv: vec4f,
    @location(3) color: vec4f,
    @location(4) halo: vec4f,
}

struct GlyphOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
    @location(2) halo: vec4f,
    @location(3) uv_rect: vec4f,
}

const ATLAS: f32 = 2048.0;
const HALO: f32 = 2.0;

@vertex
fn vs_glyph(@builtin(vertex_index) vi: u32, t: GlyphIn) -> GlyphOut {
    var corners = array<vec2f, 4>(vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0), vec2f(1.0, 1.0));
    let c = corners[vi];
    let pad = select(0.0, HALO * g.scale + 1.0, t.halo.a > 0.0);
    let p = t.pos - vec2f(pad) + c * (t.size + vec2f(pad * 2.0));
    var out: GlyphOut;
    out.pos = to_clip(p);
    let lo = t.uv.xy - vec2f(pad);
    let hi = t.uv.zw + vec2f(pad);
    out.uv = mix(lo, hi, c) / ATLAS;
    out.color = t.color;
    out.halo = t.halo;
    out.uv_rect = t.uv / ATLAS;
    return out;
}

fn atlas_at(uv: vec2f, rect: vec4f) -> f32 {
    // Stay inside this glyph's slot so neighbours in the atlas never bleed in.
    if (uv.x < rect.x || uv.y < rect.y || uv.x > rect.z || uv.y > rect.w) {
        return 0.0;
    }
    return textureSampleLevel(tex, samp, uv, 0.0).r;
}

@fragment
fn fs_glyph(in: GlyphOut) -> @location(0) vec4f {
    let a = atlas_at(in.uv, in.uv_rect);
    // Slight gamma lift keeps thin strokes solid against saturated grounds.
    let ink = pow(a, 0.82) * in.color.a;
    if (in.halo.a <= 0.0) {
        return vec4f(in.color.rgb * ink, ink);
    }
    let r = HALO * g.scale / ATLAS;
    var halo = 0.0;
    for (var i = 0; i < 8; i += 1) {
        let angle = f32(i) * 0.7853982;
        let o = vec2f(cos(angle), sin(angle)) * r;
        halo = max(halo, atlas_at(in.uv + o, in.uv_rect));
        halo = max(halo, atlas_at(in.uv + o * 0.5, in.uv_rect));
    }
    let h = clamp(max(halo, a) * 1.7, 0.0, 1.0) * in.halo.a * 0.92;
    return vec4f(in.halo.rgb * h, h);
}

// ---------------------------------------------------------------- image preview

struct ImageIn {
    @location(0) rect: vec4f,
    @location(1) radius: f32,
}

struct ImageOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) p: vec2f,
    @location(2) rect: vec4f,
    @location(3) radius: f32,
}

@vertex
fn vs_image(@builtin(vertex_index) vi: u32, r: ImageIn) -> ImageOut {
    var corners = array<vec2f, 4>(vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(0.0, 1.0), vec2f(1.0, 1.0));
    let c = corners[vi];
    let p = mix(r.rect.xy, r.rect.zw, c);
    var out: ImageOut;
    out.pos = to_clip(p);
    out.uv = c;
    out.p = p;
    out.rect = r.rect;
    out.radius = r.radius;
    return out;
}

@fragment
fn fs_image(in: ImageOut) -> @location(0) vec4f {
    let center = (in.rect.xy + in.rect.zw) * 0.5;
    let half_size = (in.rect.zw - in.rect.xy) * 0.5;
    let q = abs(in.p - center) - half_size + vec2f(in.radius);
    let d = length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - in.radius;
    let a = clamp(0.5 - d, 0.0, 1.0);
    let c = textureSampleLevel(tex, samp, in.uv, 0.0);
    return vec4f(c.rgb * c.a * a, c.a * a);
}
