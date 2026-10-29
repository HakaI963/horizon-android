//! The launcher's pixel art, ported from the desktop launcher's `launcher/src/art.rs`.
//!
//! The desktop draws two things rather than shipping images: the stone texture behind
//! every page, and the landscape behind the Play page's banner. Both are generated
//! from a small fixed random sequence, so the art is the same on every start and on
//! every device, and neither costs an asset.
//!
//! The one difference is where it is generated. GPUI bakes an image at the window's
//! scale and draws it at exactly that size, so its texels are whole device pixels.
//! imgui samples a texture with a sampler of its own, so these are uploaded with
//! nearest-neighbour sampling and drawn at a whole multiple of their texel size,
//! which is the same guarantee by a different route.

/// The stone texture comes in squares of this many logical pixels (4 by 4 tiles).
pub const STONE: f32 = horizon_theme::TILE_TEXEL * (horizon_theme::TILE_TEXELS * 4) as f32;
/// The landscape's texels are this many logical pixels, and it is this many texels tall.
pub const SCENE_TEXEL: f32 = 6.0;
pub const SCENE_HEIGHT: usize = 56;

/// A small, fixed random sequence (xorshift), so the art is the same on every start.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1 << 24) as f32
    }
}

fn shade(c: u32, by: i16) -> u32 {
    [16, 8, 0].iter().fold(0, |out, s| {
        let v = (((c >> s) & 255) as i16 + by).clamp(0, 255) as u32;
        out | v << s
    })
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    [16, 8, 0].iter().fold(0, |out, s| {
        let (x, y) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        out | ((x + (y - x) * t).round() as u32) << s
    })
}

/// `n` by `n` texels of stone: the page colour in a few close shades, with darker flecks.
pub fn stone(page: u32, n: usize) -> Vec<u32> {
    let shades = horizon_theme::TILE_SHADES.map(|by| shade(page, by));
    let mut rng = Rng(0x9e37_79b9);
    let mut out: Vec<u32> = (0..n * n)
        .map(|_| {
            let v = rng.next();
            shades[if v < 0.5 {
                0
            } else if v < 0.78 {
                1
            } else if v < 0.93 {
                2
            } else {
                3
            }]
        })
        .collect();
    for _ in 0..n * n / 50 {
        let (x, y) = (
            (rng.next() * n as f32) as usize,
            (rng.next() * n as f32) as usize,
        );
        let len = 1 + (rng.next() * 3.) as usize;
        for i in 0..len {
            out[y * n + (x + i) % n] = shades[4];
            out[((y + 1) % n) * n + (x + i + 1) % n] = shades[4];
        }
    }
    out
}

/// A landscape's colours: the sky from the top down, the sun's bands, then the hills
/// from far to near, the near hill's top edge, and the trees.
pub struct Palette {
    sky: &'static [u32],
    sun: [u32; 3],
    far: u32,
    mid: u32,
    near: u32,
    top: u32,
    trees: u32,
}

/// The sunset of Horizon's first icon: night blue to violet to coral, and a sun in
/// three bands.
pub const SUNSET: Palette = Palette {
    sky: &[
        0x0a0f2c, 0x26206a, 0x4b2a7e, 0x7c3a88, 0xc4527a, 0xee6e5a, 0xffb25e,
    ],
    sun: [0xfff6cf, 0xffd272, 0xff9a44],
    far: 0x4a2f7e,
    mid: 0x2b1f5e,
    near: 0x151a45,
    top: 0x3d2f80,
    trees: 0x0e1236,
};

/// The light appearance's morning: lavender to peach, soft violet hills.
pub const DAWN: Palette = Palette {
    sky: &[
        0x8d7fd8, 0xa596e2, 0xbcaeea, 0xd2c4f0, 0xe7d2ee, 0xf6dce4, 0xffe6d6,
    ],
    sun: [0xffffff, 0xfff1d0, 0xffd9a8],
    far: 0xa898dc,
    mid: 0x8676c8,
    near: 0x6a5ab4,
    top: 0x9a8ae0,
    trees: 0x4c3e94,
};

/// The sky's top colour, which is what the banner is filled with before the landscape.
pub fn sky_top(dark: bool) -> u32 {
    if dark { SUNSET.sky[0] } else { DAWN.sky[0] }
}

/// A blocky landscape, `w` by `h` texels: banded sky, a sun half set behind the far
/// hills and cut into slices like Horizon's mark, flat clouds, three ridges of hills
/// and a few square trees. Each part draws from its own generator from left to right,
/// so a wider landscape only adds columns: the first `w` are the same at every width.
pub fn scene(p: &Palette, seed: u32, w: usize, h: usize) -> Vec<u32> {
    let mut seeds = Rng(seed.wrapping_mul(0x2545_f491) | 1);
    let mut rng = || {
        seeds.next();
        Rng(seeds.0)
    };
    let mut out = vec![0; w * h];
    let horizon = h * 62 / 100;
    let band = horizon as f32 / p.sky.len() as f32;
    for y in 0..h {
        let f = y as f32 / band;
        let i = (f as usize).min(p.sky.len() - 1);
        let dither = f - i as f32 > 0.88 && i + 1 < p.sky.len();
        for x in 0..w {
            out[y * w + x] = if dither && (x + y) % 2 == 1 {
                p.sky[i + 1]
            } else {
                p.sky[i]
            };
        }
    }
    let rect = |out: &mut Vec<u32>, x: isize, y: isize, rw: usize, rh: usize, c: u32| {
        for yy in y.max(0) as usize..((y + rh as isize).max(0) as usize).min(h) {
            for xx in x.max(0) as usize..((x + rw as isize).max(0) as usize).min(w) {
                out[yy * w + xx] = c;
            }
        }
    };
    // Four clouds in every stretch of `CLOUDS` texels.
    const CLOUDS: usize = 220;
    let mut clouds = rng();
    for stretch in 0..w.div_ceil(CLOUDS) {
        for _ in 0..4 {
            let cx = (stretch * CLOUDS) as isize + (clouds.next() * CLOUDS as f32) as isize;
            let cy = (horizon as f32 * (0.12 + clouds.next() * 0.4)) as isize;
            let cw = 10 + (clouds.next() * 14.) as usize;
            let under = p.sky[((cy as f32 / band) as usize + 1).min(p.sky.len() - 1)];
            let c = mix(under, 0xffffff, 0.12);
            rect(&mut out, cx, cy, cw, 2, c);
            rect(&mut out, cx + 3, cy - 2, cw.saturating_sub(6), 2, c);
        }
    }
    // The sun, where it was when the landscape had one width.
    let r = (h as f32 * 0.2) as isize;
    let (cx, cy) = (149, horizon as isize - (h as f32 * 0.11) as isize);
    for dy in -r..=0 {
        let half = ((r * r - dy * dy) as f32).sqrt().round() as isize;
        let t = (dy + r) as f32 / r as f32;
        let c = p.sun[if t < 0.4 {
            0
        } else if t < 0.75 {
            1
        } else {
            2
        }];
        if (cy + dy) % 5 != 0 || (dy as f32) < -r as f32 * 0.6 {
            rect(&mut out, cx - half, cy + dy, (half * 2) as usize, 1, c);
        }
    }
    let ridge = |mut rng: Rng, base: f32, amp: f32, rough: f32| -> Vec<usize> {
        let (mut y, mut v) = (base, 0.);
        (0..w)
            .map(|x| {
                if x % 2 == 0 {
                    v = (v + (rng.next() - 0.5) * rough) * 0.86;
                    y += v + (base - y) * 0.06;
                }
                y.clamp(base - amp, base + amp).round().max(0.) as usize
            })
            .collect()
    };
    let hf = h as f32;
    let layers = [
        (p.far, horizon as f32 - hf * 0.1, hf * 0.12, 3.2, None),
        (p.mid, horizon as f32 + hf * 0.02, hf * 0.09, 2.6, None),
        (
            p.near,
            horizon as f32 + hf * 0.18,
            hf * 0.07,
            2.2,
            Some(p.top),
        ),
    ];
    let mut near = Vec::new();
    for (c, base, amp, rough, top) in layers {
        let hs = ridge(rng(), base, amp, rough);
        for (x, &y) in hs.iter().enumerate() {
            for yy in y.min(h)..h {
                out[yy * w + x] = c;
            }
            if let (Some(top), true) = (top, y < h) {
                out[y * w + x] = top;
            }
        }
        near = hs;
    }
    // A tree somewhere in every 26 texels of the near hills.
    let mut trees = rng();
    for cell in 0..w.div_ceil(26) {
        let tx = cell * 26 + 2 + (trees.next() * 22.) as usize;
        let Some(&ty) = near.get(tx) else { break };
        let (tx, ty) = (tx as isize, ty as isize);
        rect(&mut out, tx - 2, ty - 8, 5, 5, p.trees);
        rect(&mut out, tx - 1, ty - 10, 3, 2, p.trees);
        rect(&mut out, tx, ty - 3, 1, 3, p.trees);
    }
    out
}

/// `texels` as RGBA8, which is what the texture upload wants.
pub fn rgba(texels: &[u32]) -> Vec<u8> {
    texels
        .iter()
        .flat_map(|c| {
            [
                ((c >> 16) & 0xff) as u8,
                ((c >> 8) & 0xff) as u8,
                (c & 0xff) as u8,
                255,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_is_the_same_on_every_start() {
        assert_eq!(scene(&SUNSET, 3, 40, 20), scene(&SUNSET, 3, 40, 20));
        assert_eq!(stone(0x0f0d17, 16), stone(0x0f0d17, 16));
    }

    #[test]
    fn the_stone_stays_within_its_shades() {
        let shades = horizon_theme::TILE_SHADES.map(|by| shade(0x0f0d17, by));
        assert!(stone(0x0f0d17, 64).iter().all(|c| shades.contains(c)));
    }

    #[test]
    fn a_wider_landscape_only_adds_columns() {
        let narrow = scene(&SUNSET, 3, 100, 56);
        let wide = scene(&SUNSET, 3, 200, 56);
        for y in 0..56 {
            for x in 0..100 {
                assert_eq!(narrow[y * 100 + x], wide[y * 200 + x]);
            }
        }
    }
}

/// A 64x64 player skin, drawn here rather than taken from the pack.
///
/// The Skins page has to show a player before the player has picked one, and before
/// the pack's atlas is even available on the bring-up card. Deriving it from the
/// atlas meant that when the sprite was missing — or the atlas had not been built
/// yet — there was no texture at all, and the page showed an empty card. This is the
/// default skin as pixels: the same regions and the same colours as the game's own
/// `steve`, laid out on Minecraft's 64x64 table.
pub fn default_skin() -> (u32, u32, Vec<u8>) {
    const W: usize = 64;
    const H: usize = 64;
    let mut px = vec![0u8; W * H * 4];
    // The palette: skin, hair, shirt, trousers, shoes, and the face's two dark tones.
    const SKIN: u32 = 0xc79a72;
    const SKIN_SHADE: u32 = 0xb08462;
    const HAIR: u32 = 0x3f2a1d;
    const SHIRT: u32 = 0x2e9c9c;
    const SHIRT_SHADE: u32 = 0x257f7f;
    const TROUSERS: u32 = 0x3b3b8f;
    const TROUSERS_SHADE: u32 = 0x30307a;
    const SHOE: u32 = 0x4a4a4a;
    const EYE: u32 = 0x2a2a2e;
    const MOUTH: u32 = 0x8a5a44;
    let mut rect = |x0: usize, y0: usize, x1: usize, y1: usize, c: u32| {
        for y in y0..y1.min(H) {
            for x in x0..x1.min(W) {
                let o = (y * W + x) * 4;
                px[o] = ((c >> 16) & 0xff) as u8;
                px[o + 1] = ((c >> 8) & 0xff) as u8;
                px[o + 2] = (c & 0xff) as u8;
                px[o + 3] = 255;
            }
        }
    };
    // Head: base 8x8x8 at (0,0), so its six faces are the block from (0,0) to (32,16).
    rect(8, 0, 16, 8, HAIR); // top
    rect(16, 0, 24, 8, SKIN_SHADE); // bottom
    rect(0, 8, 8, 16, HAIR); // right side
    rect(8, 8, 16, 16, SKIN); // front
    rect(16, 8, 24, 16, HAIR); // left side
    rect(24, 8, 32, 16, HAIR); // back
    // The face: hairline, two eyes, a mouth, on the front face at (8,8)-(16,16).
    rect(8, 8, 16, 10, HAIR);
    rect(9, 11, 11, 13, EYE);
    rect(13, 11, 15, 13, EYE);
    rect(10, 14, 14, 15, MOUTH);
    // Body: 8x12x4 at (16,16), so (16,16) to (40,32).
    rect(20, 16, 28, 20, SHIRT); // top
    rect(28, 16, 36, 20, SHIRT_SHADE); // bottom
    rect(16, 20, 20, 32, SHIRT_SHADE); // right
    rect(20, 20, 28, 32, SHIRT); // front
    rect(28, 20, 32, 32, SHIRT_SHADE); // left
    rect(32, 20, 40, 32, SHIRT); // back
    // Right arm: 4x12x4 at (40,16).
    rect(44, 16, 48, 20, SKIN);
    rect(48, 16, 52, 20, SKIN_SHADE);
    rect(40, 20, 44, 32, SKIN_SHADE);
    rect(44, 20, 48, 32, SKIN);
    rect(48, 20, 52, 32, SKIN_SHADE);
    rect(52, 20, 56, 32, SKIN);
    // Left arm: 4x12x4 at (32,48).
    rect(36, 48, 40, 52, SKIN);
    rect(40, 48, 44, 52, SKIN_SHADE);
    rect(32, 52, 36, 64, SKIN_SHADE);
    rect(36, 52, 40, 64, SKIN);
    rect(40, 52, 44, 64, SKIN_SHADE);
    rect(44, 52, 48, 64, SKIN);
    // Right leg: 4x12x4 at (0,16).
    rect(4, 16, 8, 20, TROUSERS);
    rect(8, 16, 12, 20, TROUSERS_SHADE);
    rect(0, 20, 4, 32, TROUSERS_SHADE);
    rect(4, 20, 8, 32, TROUSERS);
    rect(8, 20, 12, 32, TROUSERS_SHADE);
    rect(12, 20, 16, 32, TROUSERS);
    // The shoes are the last two rows of each leg's side faces.
    rect(0, 29, 4, 32, SHOE);
    rect(4, 29, 8, 32, SHOE);
    rect(8, 29, 12, 32, SHOE);
    rect(12, 29, 16, 32, SHOE);
    // Left leg: 4x12x4 at (16,48).
    rect(20, 48, 24, 52, TROUSERS);
    rect(24, 48, 28, 52, TROUSERS_SHADE);
    rect(16, 52, 20, 64, TROUSERS_SHADE);
    rect(20, 52, 24, 64, TROUSERS);
    rect(24, 52, 28, 64, TROUSERS_SHADE);
    rect(28, 52, 32, 64, TROUSERS);
    rect(16, 61, 20, 64, SHOE);
    rect(20, 61, 24, 64, SHOE);
    rect(24, 61, 28, 64, SHOE);
    rect(28, 61, 32, 64, SHOE);
    (W as u32, H as u32, px)
}

#[cfg(test)]
mod skin_tests {
    use super::*;

    #[test]
    fn every_face_the_model_draws_is_painted() {
        let (w, h, px) = default_skin();
        assert_eq!((w, h), (64, 64));
        assert_eq!(px.len(), 64 * 64 * 4);
        // The six faces of each part, as the model reads them. A 64x64 skin leaves
        // padding around the head's top and bottom faces, so the whole square is not
        // painted; these rectangles are what the figure actually samples.
        let faces = [
            (8, 0, 16, 8), (16, 0, 24, 8), (0, 8, 8, 16),
            (8, 8, 16, 16), (16, 8, 24, 16), (24, 8, 32, 16),
            (20, 16, 28, 20), (28, 16, 36, 20), (16, 20, 20, 32),
            (20, 20, 28, 32), (28, 20, 32, 32), (32, 20, 40, 32),
            (44, 16, 48, 20), (48, 16, 52, 20), (40, 20, 44, 32),
            (44, 20, 48, 32), (48, 20, 52, 32), (52, 20, 56, 32),
            (36, 48, 40, 52), (40, 48, 44, 52), (32, 52, 36, 64),
            (36, 52, 40, 64), (40, 52, 44, 64), (44, 52, 48, 64),
            (4, 16, 8, 20), (8, 16, 12, 20), (0, 20, 4, 32),
            (4, 20, 8, 32), (8, 20, 12, 32), (12, 20, 16, 32),
            (20, 48, 24, 52), (24, 48, 28, 52), (16, 52, 20, 64),
            (20, 52, 24, 64), (24, 52, 28, 64), (28, 52, 32, 64),
        ];
        for (x0, y0, x1, y1) in faces {
            for y in y0..y1 {
                for x in x0..x1 {
                    let o = ((y * 64 + x) * 4) as usize;
                    assert_eq!(px[o + 3], 255, "texel {x},{y} is transparent");
                }
            }
        }
    }
}
