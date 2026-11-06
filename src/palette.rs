//! The colours of the paper. All colours in the app come from here.

pub const GROUND: u32 = 0x1f3fb3;
pub const GROUND_DEEP: u32 = 0x15308f;
pub const SOIL: u32 = 0x128a58;
pub const SHADOW: u32 = 0x06103a;

pub const BAR: u32 = 0x16307f;
pub const BAR_HOVER: u32 = 0x24449f;
pub const CREAM: u32 = 0xfff7e6;
pub const BRANCH: u32 = 0xfff0cf;
/// Where a branch's paper recedes to when it isn't in focus: still warm.
pub const BRANCH_DIM: u32 = 0xc4ad85;
pub const FOCUS_RING: u32 = 0x9db4ff;
pub const INK: u32 = 0x14205c;
pub const INK_SOFT: u32 = 0x4a5484;
pub const PANEL_HOVER: u32 = 0xf6e9cf;
pub const PREVIEW_BG: u32 = 0xeadfc4;
/// Only for actions that remove something, and for things that went wrong.
pub const DANGER: u32 = 0xc8321b;
pub const DANGER_HOVER: u32 = 0xdc4630;
pub const DANGER_INK: u32 = 0xb02a14;
/// Source code on PREVIEW_BG.
pub const CODE_COMMENT: u32 = 0x6b6a78;
pub const CODE_STRING: u32 = 0x0b6e60;
pub const CODE_KEYWORD: u32 = 0x8a1f8f;
pub const CODE_NUMBER: u32 = 0xa8430a;

/// Reserved: the path to the selected item, and nothing else on the tree.
pub const PATH: u32 = 0xffe45c;

/// Outer petals, inner petals, centre, in `Species` order, then leaf and bud.
pub const SPECIES: [[u32; 3]; 13] = [
    [0xfff7e6, 0xfff7e6, 0xf7a11a], // daisy: documents
    [0xf0325b, 0xff8fb1, 0xfff7e6], // poppy: images
    [0xd9390f, 0xff8a2a, 0x14205c], // trillium: video
    [0x12a79a, 0x7ee0d0, 0xfff7e6], // bellflower: audio
    [0xffb3e6, 0xffb3e6, 0x14205c], // star: code
    [0xbdf06b, 0xbdf06b, 0x14205c], // quince: data
    [0xa873f5, 0xd6b5ff, 0xfff7e6], // allium: archives
    [0xf26b3a, 0xffd0a8, 0xfff7e6], // dahlia: apps
    [0x8fd3ff, 0x8fd3ff, 0x14205c], // clover: everything else
    [0x26a269, 0x8be0ae, 0x000000], // leaf
    [0xff8a2a, 0x26a269, 0x000000], // bud
    [0x000000, 0x000000, 0x000000], // shed petals borrow their flower's colours
    [0xff8fb1, 0xfff7e6, 0xff8a2a], // blossom: a whole disk
];

pub const LEAF: f32 = 9.0;
pub const BUD: f32 = 10.0;
pub const PETAL: f32 = 11.0;
pub const BLOSSOM: f32 = 12.0;
