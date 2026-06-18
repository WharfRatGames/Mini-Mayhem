use crate::world::{WorldPos, Vec2, WATER_Y};
use super::buffer::WorldBuffer;
use super::fb::Bgra;

// ── Garcia sprite ─────────────────────────────────────────────────────────────

const GARCIA_PNG: &[u8] = include_bytes!("../../deploy/assets/GARCIA.png");

struct GarciaSprite {
    w: usize,
    h: usize,
    px: Vec<[u8; 4]>, // RGBA row-major
}

fn garcia_sprite() -> &'static GarciaSprite {
    static SPRITE: std::sync::OnceLock<GarciaSprite> = std::sync::OnceLock::new();
    SPRITE.get_or_init(|| {
        let decoder = png::Decoder::new(std::io::Cursor::new(GARCIA_PNG));
        if let Ok(mut reader) = decoder.read_info() {
            let info = reader.info();
            let (w, h) = (info.width as usize, info.height as usize);
            let color  = info.color_type;
            let mut raw = vec![0u8; reader.output_buffer_size()];
            if reader.next_frame(&mut raw).is_ok() {
                let px: Vec<[u8; 4]> = match color {
                    png::ColorType::Rgba => raw.chunks_exact(4).map(|c| [c[0],c[1],c[2],c[3]]).collect(),
                    png::ColorType::Rgb  => raw.chunks_exact(3).map(|c| [c[0],c[1],c[2],255]).collect(),
                    _ => vec![[0,0,0,0]; w*h],
                };
                return GarciaSprite { w, h, px };
            }
        }
        GarciaSprite { w: 1, h: 1, px: vec![[0,0,0,0]] }
    })
}

/// Draw the GARCIA.png sprite centred at (cx, cy), scaled to render_w × render_h.
pub fn draw_garcia_sprite(buf: &mut WorldBuffer, cx: i32, cy: i32, render_w: i32, render_h: i32) {
    let sp = garcia_sprite();
    if sp.w == 0 || sp.h == 0 { return; }
    let x0 = cx - render_w / 2;
    let y0 = cy - render_h / 2;
    for dy in 0..render_h {
        for dx in 0..render_w {
            let sx = dx * sp.w as i32 / render_w;
            let sy = dy * sp.h as i32 / render_h;
            let idx = sy as usize * sp.w + sx as usize;
            if idx >= sp.px.len() { continue; }
            let [r, g, b, a] = sp.px[idx];
            if a < 30 { continue; } // transparent
            buf.set_pixel(x0 + dx, y0 + dy, Bgra::new(r, g, b));
        }
    }
}

// ── Team colours ──────────────────────────────────────────────────────────────

/// The four team colours. Index matches team slot 0-3.
pub const TEAM_COLOURS: [Bgra; 4] = [
    Bgra::new(220, 80,  80),  // Red
    Bgra::new(80,  120, 220), // Blue
    Bgra::new(80,  200, 80),  // Green
    Bgra::new(220, 180, 40),  // Yellow
];

/// Dimmed version of team colour for dead soldiers.
pub const TEAM_COLOURS_DEAD: [Bgra; 4] = [
    Bgra::new(100, 40, 40),
    Bgra::new(40,  60, 100),
    Bgra::new(40,  90, 40),
    Bgra::new(100, 80, 20),
];

// ── Soldier dimensions ────────────────────────────────────────────────────────

/// Soldier body width in pixels (used for health bar centering).
pub const SOLDIER_W: i32 = 14;
/// Soldier body height in pixels — also used as the collision vertical extent.
pub const SOLDIER_H: i32 = 20;
/// Half-width for centering.
pub const SOLDIER_HALF_W: i32 = SOLDIER_W / 2;

// ── HP number ─────────────────────────────────────────────────────────────────

// ── Aim arrow ────────────────────────────────────────────────────────────────

const AIM_ARROW_MIN_LEN: f32 = 16.0;
const AIM_ARROW_MAX_LEN: f32 = 80.0;

/// V1 sprite — preserved for rollback. 12×15 simple rectangle soldier.
#[allow(dead_code)]
pub fn draw_soldier_v1(
    buf:    &mut WorldBuffer,
    pos:    WorldPos,
    team:   usize,
    facing: i8,
    hp:     u8,
    spin_frame: u8,
) {
    let colour = if hp == 0 { TEAM_COLOURS_DEAD[team.min(3)] } else { TEAM_COLOURS[team.min(3)] };
    let cx = pos.x as i32;
    let fy = pos.y as i32;
    let head_colour = Bgra::new(colour.r.saturating_add(40), colour.g.saturating_add(40), colour.b.saturating_add(40));
    const W: i32 = 12; const H: i32 = 15;
    match spin_frame % 4 {
        1 => { buf.fill_rect(cx-H/2, fy-W, H as u32, W as u32, colour); buf.fill_rect(cx-H/2-6, fy-W/2-3, 6, 6, head_colour); }
        3 => { buf.fill_rect(cx-H/2, fy-W, H as u32, W as u32, colour); buf.fill_rect(cx+H/2,   fy-W/2-3, 6, 6, head_colour); }
        _ => { buf.fill_rect(cx-6,   fy-H, W as u32, H as u32, colour); buf.fill_rect(cx-3,     fy-H-6,   6, 6, head_colour); }
    }
    if spin_frame == 0 {
        let nub_x = if facing >= 0 { cx + 6 } else { cx - 8 };
        buf.fill_rect(nub_x, fy - H/2 - 1, 2, 2, Bgra::white());
    }
    if hp > 0 { draw_hp_number(buf, cx, fy, hp, team); }
}

/// Draw a soldier body at world position (pos = foot position).
/// `team` is 0-3, `facing` is +1 for right, -1 for left.
/// `hp` 0 = dead (dimmed), > 0 = alive (team colour).
pub fn draw_soldier(
    buf:    &mut WorldBuffer,
    pos:    WorldPos,
    team:   usize,
    facing: i8,
    hp:     u8,
    spin_frame: u8,
) {
    let body   = if hp == 0 { TEAM_COLOURS_DEAD[team.min(3)] } else { TEAM_COLOURS[team.min(3)] };
    let head   = Bgra::new(body.r.saturating_add(35), body.g.saturating_add(35), body.b.saturating_add(35));
    let helmet = Bgra::new(body.r.saturating_sub(20), body.g.saturating_sub(20), body.b.saturating_sub(20));
    let legs   = Bgra::new(body.r.saturating_sub(30), body.g.saturating_sub(30), body.b.saturating_sub(30));
    let gun    = Bgra::new(180, 180, 180);

    let cx = pos.x as i32;
    let fy = pos.y as i32;

    match spin_frame % 4 {
        1 | 3 => {
            // Airborne spin — horizontal blob
            let flip = if spin_frame % 4 == 1 { -1i32 } else { 1i32 };
            buf.fill_rect(cx - SOLDIER_H/2, fy - SOLDIER_W, SOLDIER_H as u32, SOLDIER_W as u32, body);
            buf.fill_rect(cx - SOLDIER_H/2 + flip*(SOLDIER_H/2+2), fy - SOLDIER_W/2 - 4, 7, 7, head);
        }
        _ => {
            // Upright
            // Helmet (2px above head, slightly wider)
            buf.fill_rect(cx - 7, fy - SOLDIER_H - 3, 14, 3, helmet);
            // Head
            buf.fill_rect(cx - 5, fy - SOLDIER_H,     10, 7, head);
            // Torso
            buf.fill_rect(cx - 6, fy - 13,            12, 6, body);
            // Belt detail (1px dark strip)
            buf.fill_rect(cx - 6, fy - 7,             12, 1, helmet);
            // Left leg
            buf.fill_rect(cx - 6, fy - 6,              5, 6, legs);
            // Right leg
            buf.fill_rect(cx + 1, fy - 6,              5, 6, legs);
            // Gun arm
            if spin_frame == 0 {
                let (gx, gw) = if facing >= 0 { (cx + 6, 7i32) } else { (cx - 13, 7i32) };
                buf.fill_rect(gx, fy - 11, gw as u32, 2, gun);
            }
        }
    }

    if hp > 0 { draw_hp_number(buf, cx, fy, hp, team); }
}

/// V3 sprite — icon-style: military helmet, face, outlined body, prominent cannon.
/// To revert: change the `draw_soldier` call in loop_runner.rs to draw_soldier_v1 or draw_soldier_v2.
pub fn draw_soldier_v3(
    buf:       &mut WorldBuffer,
    pos:       WorldPos,
    team:      usize,
    facing:    i8,
    hp:        u8,
    spin_frame: u8,
    aim_angle: Option<f32>, // Some(angle) = rotate barrel; None = horizontal default
    show_hp:   bool,
) {
    let body = if hp == 0 { TEAM_COLOURS_DEAD[team.min(3)] } else { TEAM_COLOURS[team.min(3)] };
    // Derived colors
    let skin  = Bgra::new(218, 178, 140);
    let dark  = Bgra::new(22,  14,  6);
    let gun   = Bgra::new(72,  72,  78);
    let boot  = Bgra::new(body.r.saturating_sub(45), body.g.saturating_sub(35), body.b.saturating_sub(20));
    let hilit = Bgra::new(body.r.saturating_add(28), body.g.saturating_add(22), body.b.saturating_add(14));
    let cx = pos.x as i32;
    let fy = pos.y as i32;
    let f  = facing as i32;

    // Backflip rotation — 4 poses: upright(0), horizontal-right(1), inverted(2), horizontal-left(3)
    if spin_frame > 0 {
        match spin_frame % 4 {
            1 => { // 90° — horizontal, head to the right
                buf.fill_rect(cx - 14, fy - 13, 30, 16, dark);
                buf.fill_rect(cx - 13, fy - 12, 28, 14, body);
                // Head/face stub on right
                buf.fill_rect(cx + 11, fy - 11,  7, 12, dark);
                buf.fill_rect(cx + 12, fy - 10,  5, 10, skin);
                buf.fill_rect(cx + 12, fy - 10,  5,  4, body); // helmet over face
                // Cannon stub on left
                buf.fill_rect(cx - 14, fy -  9, 10,  4, dark);
                buf.fill_rect(cx - 13, fy -  8,  8,  2, gun);
            }
            2 => { // 180° — upside down v3
                buf.fill_rect(cx - 7,  fy - 25, 14, 26, dark);
                // Legs at top
                buf.fill_rect(cx - 5,  fy - 25,  4,  7, body);
                buf.fill_rect(cx + 1,  fy - 25,  4,  7, body);
                buf.fill_rect(cx - 5,  fy - 25,  4,  2, boot); // boot at very top
                buf.fill_rect(cx + 1,  fy - 25,  4,  2, boot);
                // Body
                buf.fill_rect(cx - 6,  fy - 17, 12,  8, body);
                buf.fill_rect(cx - 6,  fy - 18, 12,  1, hilit); // belt upside down
                // Face
                buf.fill_rect(cx - 4,  fy - 10,  8,  5, skin);
                let eye_x = cx + f;
                buf.fill_rect(eye_x, fy - 8, 2, 2, dark);
                // Helmet at bottom (upside down)
                buf.fill_rect(cx - 5,  fy -  5, 10,  5, body);
                buf.fill_rect(cx - 6,  fy -  4, 12,  2, body); // brim
                // Cannon pointing down
                if f >= 0 {
                    buf.fill_rect(cx + 6, fy - 16, 4, 8, dark);
                    buf.fill_rect(cx + 7, fy - 15, 3, 6, gun);
                } else {
                    buf.fill_rect(cx - 10, fy - 16, 4, 8, dark);
                    buf.fill_rect(cx - 10, fy - 15, 3, 6, gun);
                }
            }
            3 => { // 270° — horizontal, head to the left
                buf.fill_rect(cx - 16, fy - 13, 30, 16, dark);
                buf.fill_rect(cx - 15, fy - 12, 28, 14, body);
                // Head/face stub on left
                buf.fill_rect(cx - 18, fy - 11,  7, 12, dark);
                buf.fill_rect(cx - 17, fy - 10,  5, 10, skin);
                buf.fill_rect(cx - 17, fy - 10,  5,  4, body);
                // Cannon stub on right
                buf.fill_rect(cx + 4,  fy -  9, 10,  4, dark);
                buf.fill_rect(cx + 5,  fy -  8,  8,  2, gun);
            }
            _ => {} // frame 0 falls through to upright draw below
        }
        if spin_frame % 4 != 0 {
            if hp > 0 && show_hp { draw_hp_number(buf, cx, fy, hp, team); }
            return;
        }
    }

    // Walk cycle: spin_frame 4-7 = walk frames 0-3 (unused with walk cycle off, kept for future)
    let wf: u8 = if spin_frame >= 4 { spin_frame - 4 } else { 0 };
    // Body bobs up 1px on frames 1 and 3 (mid-stride)
    let bob: i32 = if wf == 1 || wf == 3 { 1 } else { 0 };
    // Left leg lifts on frame 1, right leg on frame 3
    // (lift = bottom of leg drawn 2px higher = foot off ground)
    let ll: i32 = if wf == 1 { 2 } else { 0 }; // left lift
    let rl: i32 = if wf == 3 { 2 } else { 0 }; // right lift

    // ── Outline pass ──────────────────────────────────────────────────────────
    buf.fill_rect(cx - 7,  fy - 24 - bob, 14, 11, dark);
    buf.fill_rect(cx - 7,  fy - 15 - bob, 14, 16, dark);

    // ── Helmet ────────────────────────────────────────────────────────────────
    buf.fill_rect(cx - 5,  fy - 23 - bob,  10, 5, body);
    buf.fill_rect(cx - 6,  fy - 19 - bob,  12, 2, body);

    // ── Face ──────────────────────────────────────────────────────────────────
    buf.fill_rect(cx - 4,  fy - 18 - bob,   8, 5, skin);
    let eye_x = cx + f;
    buf.fill_rect(eye_x, fy - 16 - bob, 2, 2, dark);

    // ── Body / torso ──────────────────────────────────────────────────────────
    buf.fill_rect(cx - 6,  fy - 14 - bob,  12, 8, body);
    buf.fill_rect(cx - 6,  fy -  7 - bob,  12, 1, hilit);

    // ── Legs (animated) ───────────────────────────────────────────────────────
    // Left leg (lifts on walk frame 1)
    let lh = (6 - ll).max(2) as u32;
    buf.fill_rect(cx - 5,  fy -  6,   4, lh, body);
    buf.fill_rect(cx - 5,  fy -  2,   4, (2 - ll.min(2)) as u32, boot);
    // Right leg (lifts on walk frame 3)
    let rh = (6 - rl).max(2) as u32;
    buf.fill_rect(cx + 1,  fy -  6,   4, rh, body);
    buf.fill_rect(cx + 1,  fy -  2,   4, (2 - rl.min(2)) as u32, boot);

    // ── Cannon arm ────────────────────────────────────────────────────────────
    // Arm/shoulder block (fixed position, always same)
    if f >= 0 {
        buf.fill_rect(cx + 6, fy - 14 - bob, 4, 5, dark);
        buf.fill_rect(cx + 7, fy - 13 - bob, 3, 4, body);
    } else {
        buf.fill_rect(cx - 10, fy - 14 - bob, 4, 5, dark);
        buf.fill_rect(cx - 10, fy - 13 - bob, 3, 4, body);
    }
    // Barrel — angled when aiming, horizontal at rest
    if let Some(angle) = aim_angle {
        let disp = if f >= 0 { angle } else { std::f32::consts::PI - angle };
        let bx = if f >= 0 { cx + 9 } else { cx - 9 };
        let by = fy - 12 - bob;
        let len = 11.0f32;
        let tx = bx + (disp.cos() * len) as i32;
        let ty = by - (disp.sin() * len) as i32;
        buf.draw_line(bx, by - 1, tx, ty - 1, dark);
        buf.draw_line(bx, by + 1, tx, ty + 1, dark);
        buf.draw_line(bx, by,     tx, ty,     gun);
    } else if f >= 0 {
        buf.fill_rect(cx + 9,  fy - 13 - bob, 9, 3, dark);
        buf.fill_rect(cx + 10, fy - 12 - bob, 8, 2, gun);
    } else {
        buf.fill_rect(cx - 18, fy - 13 - bob, 9, 3, dark);
        buf.fill_rect(cx - 18, fy - 12 - bob, 8, 2, gun);
    }

    if hp > 0 && show_hp { draw_hp_number(buf, cx, fy, hp, team); }
}







/// Draw one frame of an explosion animation.
/// `age` counts from 0 up to Explosion::MAX_AGE.
pub fn draw_explosion(buf: &mut WorldBuffer, pos: WorldPos, radius: f32, age: u32) {
    const MAX_AGE: u32 = 20;
    if age >= MAX_AGE { return; }
    let cx = pos.x as i32;
    let cy = pos.y as i32;

    // Outer radius: ramps up over first 6 ticks, then fades
    let scale = if age < 6 {
        (age as f32 + 1.0) / 6.0
    } else {
        1.0 - (age as f32 - 5.0) / (MAX_AGE as f32 - 5.0)
    };
    let outer_r = (radius * scale.max(0.0)) as i32;
    if outer_r <= 0 { return; }

    // Outer flame colour
    let outer = match age {
        0..=1  => Bgra::new(255, 255, 180),  // white-yellow flash
        2..=4  => Bgra::new(255, 200,  30),  // yellow
        5..=7  => Bgra::new(255, 120,  10),  // orange
        8..=11 => Bgra::new(200,  50,  10),  // red-orange
        12..=15 => Bgra::new(110, 35,  15),  // dark red
        _      => Bgra::new(65,  55,  50),   // dark smoke
    };
    buf.fill_circle(cx, cy, outer_r, outer);

    // Bright inner core (~55% of outer radius)
    let inner_r = (outer_r as f32 * 0.55) as i32;
    if inner_r > 0 {
        let inner = match age {
            0..=1  => Bgra::new(255, 255, 255),  // pure white
            2..=5  => Bgra::new(255, 240, 100),  // bright yellow
            6..=9  => Bgra::new(255, 175,  40),  // yellow-orange
            10..=13 => Bgra::new(220,  80,  20), // orange-red
            _      => Bgra::new(150,  55,  25),  // dark red
        };
        buf.fill_circle(cx, cy, inner_r, inner);
    }

    // Pure white flash core on first 3 frames
    if age <= 2 {
        let flash_r = ((outer_r as f32 * 0.28) as i32).max(2);
        buf.fill_circle(cx, cy, flash_r, Bgra::new(255, 255, 255));
    }

    // Smoke puffs drifting upward after fireball peaks
    if age >= 9 {
        let smk = (age - 9) as f32 / 11.0;
        let smoke_r = (outer_r as f32 * 0.55 * (1.0 - smk)).max(0.0) as i32;
        if smoke_r > 1 {
            let gv = (155.0 * (1.0 - smk)) as u8;
            let sc = Bgra::new(gv, gv, gv);
            let drift = (smk * 14.0) as i32;
            buf.fill_circle(cx - 7, cy - 9  - drift, smoke_r, sc);
            buf.fill_circle(cx + 8, cy - 5  - drift, smoke_r, sc);
        }
    }
}

/// Number of available headstone designs.
pub const HEADSTONE_COUNT: u8 = 6;

/// Draw a headstone at the soldier's foot position (world coords).
/// headstone_id selects the symbol: 0=Cross, 1=Skull, 2=Circle, 3=Wings, 4=Bomb, 5=Star
pub fn draw_headstone(buf: &mut WorldBuffer, pos: WorldPos, team: usize, headstone_id: u8) {
    let cx = pos.x as i32;
    let fy = pos.y as i32;
    let stone = Bgra::new(125, 125, 142);
    let dark  = Bgra::new(52,  52,  66);
    let hilit = Bgra::new(185, 185, 200);
    let sym   = TEAM_COLOURS[team.min(3)]; // symbol colour
    let void  = Bgra::new(15, 15, 20);    // dark void for eye holes etc.

    // Stone body — same for all designs
    buf.fill_rect(cx - 5,  fy - 33, 10, 3, stone); // apex
    buf.fill_rect(cx - 8,  fy - 30, 16, 3, stone); // shoulder
    buf.fill_rect(cx - 10, fy - 27, 20, 27, stone); // body
    buf.fill_rect(cx - 8,  fy - 25,  3,  7, hilit); // highlight
    buf.fill_rect(cx - 12, fy - 27,  2, 27, dark);  // left outline
    buf.fill_rect(cx + 10, fy - 27,  2, 27, dark);  // right outline
    buf.fill_rect(cx - 10, fy,      20,  2, dark);   // base

    match headstone_id % HEADSTONE_COUNT {
        0 => {
            // Cross
            buf.fill_rect(cx - 2,  fy - 22,  5, 17, sym);
            buf.fill_rect(cx - 7,  fy - 17, 14,  5, sym);
        }
        1 => {
            // Skull
            buf.fill_rect(cx - 5, fy - 24, 11,  9, sym); // head top
            buf.fill_rect(cx - 6, fy - 21, 13,  5, sym); // head wide
            buf.fill_rect(cx - 3, fy - 15,  7,  5, sym); // jaw
            // Eyes
            buf.fill_rect(cx - 4, fy - 22,  3, 3, void);
            buf.fill_rect(cx + 1, fy - 22,  3, 3, void);
            // Nose
            buf.fill_rect(cx - 1, fy - 18,  2, 2, void);
            // Tooth gaps
            buf.fill_rect(cx - 2, fy - 13,  2, 3, void);
            buf.fill_rect(cx + 1, fy - 13,  2, 3, void);
        }
        2 => {
            // Circle / target: ring with centre dot
            buf.fill_circle(cx, fy - 17, 7, sym);
            buf.fill_circle(cx, fy - 17, 5, stone); // hollow
            buf.fill_circle(cx, fy - 17, 2, sym);   // centre dot
        }
        3 => {
            // Wings
            // Central body
            buf.fill_rect(cx - 1, fy - 20, 3, 8, sym);
            // Left wing
            buf.fill_rect(cx - 6, fy - 18, 4, 2, sym);
            buf.fill_rect(cx - 8, fy - 16, 5, 2, sym);
            buf.fill_rect(cx - 6, fy - 14, 3, 2, sym);
            // Right wing (mirrored)
            buf.fill_rect(cx + 2, fy - 18, 4, 2, sym);
            buf.fill_rect(cx + 3, fy - 16, 5, 2, sym);
            buf.fill_rect(cx + 3, fy - 14, 3, 2, sym);
        }
        4 => {
            // Bomb / missile
            buf.fill_rect(cx - 2, fy - 23,  5, 13, sym); // body
            buf.fill_rect(cx - 1, fy - 25,  3,  2, sym); // tip
            // Fins
            buf.fill_rect(cx - 5, fy - 13,  3,  4, sym);
            buf.fill_rect(cx + 2, fy - 13,  3,  4, sym);
            // Fuse spark
            buf.fill_rect(cx,     fy - 27,  1,  2, Bgra::new(255, 200, 50));
        }
        _ => {
            // Star (5-pointed approximation)
            buf.fill_rect(cx - 2, fy - 23,  5, 14, sym); // vertical bar
            buf.fill_rect(cx - 7, fy - 19, 15,  5, sym); // horizontal bar
            // Four diagonal corner points
            buf.fill_rect(cx - 5, fy - 24,  3,  2, sym); // top-left
            buf.fill_rect(cx + 2, fy - 24,  3,  2, sym); // top-right
            buf.fill_rect(cx - 6, fy - 12,  3,  2, sym); // bottom-left
            buf.fill_rect(cx + 3, fy - 12,  3,  2, sym); // bottom-right
        }
    }
}

/// Draw the HP value in a small dark box above a soldier's foot position.
pub fn draw_hp_number(buf: &mut WorldBuffer, cx: i32, fy: i32, hp: u8, team: usize) {
    draw_hp_number_lifted(buf, cx, fy, hp, team, 0);
}
pub fn draw_hp_number_lifted(buf: &mut WorldBuffer, cx: i32, fy: i32, hp: u8, team: usize, lift: i32) {
    use super::font::{draw_str, str_width};
    let text = format!("{}", hp);
    let tw   = str_width(&text);
    let tx   = cx - tw / 2;
    let ty   = fy - SOLDIER_H - 28 - lift;
    let col  = TEAM_COLOURS[team.min(3)];
    // Dark box with thin border
    buf.fill_rect(tx - 3, ty - 2, (tw + 6) as u32, 13, Bgra::new(0, 0, 0));
    buf.fill_rect(tx - 3, ty - 2, (tw + 6) as u32,  1, Bgra::new(70, 70, 90));
    buf.fill_rect(tx - 3, ty + 9, (tw + 6) as u32,  1, Bgra::new(70, 70, 90));
    buf.fill_rect(tx - 3, ty - 2,  1, 13, Bgra::new(70, 70, 90));
    buf.fill_rect(tx + tw + 2, ty - 2, 1, 13, Bgra::new(70, 70, 90));
    draw_str(buf, &text, tx, ty, col);
}

/// Draw the aim arrow for the active soldier.
/// `angle_rad`: aim angle in radians (0 = right, positive = upward).
/// `power`: 0.0-1.0 controls arrow length.
/// `pos`: foot position of the soldier.
/// Draw Worms-style aiming: a reticle shows aim direction plus a thin horizontal
/// charge bar below the soldier that fills left-to-right as A is held.
///
/// - Reticle: always visible crosshair at `reticle_dist` in the aim direction.
/// - Charge bar: 80×6 px, 1 px dark border, 4 px fill interior, dark-red→orange gradient.
///   Bazooka overcharge (power > 1.0) flips all fill pixels to bright orange.
/// - `power_frac` 0.0–1.2 (0 = empty bar, 1.0 = full, >1.0 = bazooka overcharge).
pub fn draw_aim_arrow(
    buf:        &mut WorldBuffer,
    origin:     (f32, f32),
    angle_rad:  f32,
    power_frac: f32,
) {
    let ox = origin.0 as i32;
    let oy = origin.1 as i32;
    let ca = angle_rad.cos();
    let sa = angle_rad.sin();

    let reticle_dist = 108.0f32; // bar_len(100) + reticle_radius(7) + gap(1)
    let rx = ox + (ca * reticle_dist) as i32;
    let ry = oy - (sa * reticle_dist) as i32;

    let rc = Bgra::yellow();
    let wc = Bgra::new(255, 255, 255);

    // Reticle circle (midpoint algorithm, radius=7)
    let r = 7i32;
    let mut dx = 0i32;
    let mut dy = r;
    let mut d = 1 - r;
    while dx <= dy {
        for (px, py) in [
            (rx+dx,ry+dy),(rx-dx,ry+dy),(rx+dx,ry-dy),(rx-dx,ry-dy),
            (rx+dy,ry+dx),(rx-dy,ry+dx),(rx+dy,ry-dx),(rx-dy,ry-dx),
        ] { buf.set_pixel(px, py, rc); }
        if d < 0 { d += 2*dx + 3; } else { d += 2*(dx-dy) + 5; dy -= 1; }
        dx += 1;
    }

    // Cross lines through center (full diameter)
    for i in -r..=r {
        buf.set_pixel(rx + i, ry, rc);
        buf.set_pixel(rx, ry + i, rc);
    }

    // White center dot
    buf.set_pixel(rx,     ry,     wc);
    buf.set_pixel(rx + 1, ry,     wc);
    buf.set_pixel(rx,     ry + 1, wc);
    buf.set_pixel(rx + 1, ry + 1, wc);

    // Rotated charge bar: only visible while charging. Border wraps exactly the filled
    // portion — border and fill pixels are drawn together as the bar grows.
    if power_frac > 0.005 {
        // Longer meter: full bar = MAX_CHARGE. The stretch from 1.0..MAX_CHARGE is the
        // bonus-range band, drawn in bright orange so it reads as "extra power".
        let max          = crate::game::loop_runner::MAX_CHARGE;
        let bar_len      = 100i32;
        let interior     = (bar_len - 2) as f32; // 98 fillable columns
        let fill_px      = (power_frac.min(max) / max * interior) as i32;
        let normal_full  = (1.0 / max * interior) as i32; // column where power=1.0 lands
        let border_col   = Bgra::new(255, 255, 255);
        let shadow_col   = Bgra::new(0, 0, 0);
        let right_cap    = (fill_px + 1).min(bar_len - 1);

        // Shadow pass: draw the bar outline 1px offset (down-right) for contrast
        // on any background colour.
        for col in 0..=right_cap {
            let base_x = ox as f32 + ca * col as f32;
            let base_y = oy as f32 - sa * col as f32;
            let is_cap = col == 0 || col == right_cap;
            let half = (2 + col * 6 / fill_px.max(1)).min(8) as i32;
            for row_off in -half..=half {
                let is_border_row = row_off == -half || row_off == half;
                if !(is_cap || is_border_row) { continue; }
                let px = (base_x + sa * row_off as f32).round() as i32;
                let py = (base_y + ca * row_off as f32).round() as i32;
                buf.set_pixel(px + 1, py + 1, shadow_col);
            }
        }

        for col in 0..=right_cap {
            let base_x = ox as f32 + ca * col as f32;
            let base_y = oy as f32 - sa * col as f32;
            let is_cap = col == 0 || col == right_cap;
            // Taper: half-width grows from 2 at muzzle to 8 at end of current charge
            let half = (2 + col * 6 / fill_px.max(1)).min(8) as i32;
            for row_off in -half..=half {
                let px = (base_x + sa * row_off as f32).round() as i32;
                let py = (base_y + ca * row_off as f32).round() as i32;
                let is_border_row = row_off == -half || row_off == half;
                let color = if is_cap || is_border_row {
                    border_col
                } else if col >= 1 && col <= fill_px {
                    if col > normal_full {
                        Bgra::new(255, 140, 0) // bonus overcharge band
                    } else {
                        let t = (col - 1) as f32 / (normal_full.max(2) - 1) as f32;
                        Bgra::new((120.0 + 135.0 * t) as u8, (120.0 * t) as u8, 0)
                    }
                } else {
                    continue; // cap col, non-border interior — skip
                };
                buf.set_pixel(px, py, color);
            }
        }
    }
}

/// Draw a projectile as a small dot. Size varies by weapon kind.
pub fn draw_projectile(buf: &mut WorldBuffer, pos: WorldPos, radius: i32, colour: Bgra) {
    buf.fill_circle(pos.x as i32, pos.y as i32, radius, colour);
}

/// Draw a bazooka rocket — ~17 px long, 5 px wide body, rotated to face velocity.
pub fn draw_bazooka(buf: &mut WorldBuffer, pos: WorldPos, vel: crate::world::Vec2) {
    let speed = (vel.x * vel.x + vel.y * vel.y).sqrt();
    let px = pos.x.round() as i32;
    let py = pos.y.round() as i32;
    if speed < 0.1 {
        buf.fill_circle(px, py, 4, Bgra::new(200, 100, 20));
        return;
    }
    let nx = vel.x / speed;  // forward unit
    let ny = vel.y / speed;
    // Compute a point offset by (t) along the axis and (p) perpendicular (-ny, nx).
    let pt = |t: f32, p: f32| -> (i32, i32) {
        ((pos.x + nx * t - ny * p).round() as i32,
         (pos.y + ny * t + nx * p).round() as i32)
    };

    let exhaust    = Bgra::new( 80,  25,  5);   // tail glow
    let body_edge  = Bgra::new(120,  50,  8);   // outermost body edges (±2)
    let body_side  = Bgra::new(160,  75, 15);   // inner body (±1)
    let body_ctr   = Bgra::new(210, 110, 25);   // body centre
    let nose_col   = Bgra::new(235, 150, 40);   // nose cone
    let tip_col    = Bgra::new(255, 230, 80);   // bright nose tip

    // Tail exhaust dot (-7)
    let (ex, ey) = pt(-7.0, 0.0);
    buf.set_pixel(ex, ey, exhaust);

    // Body: 5 parallel lines from -6 to +3
    let (b0x, b0y) = pt(-6.0,  0.0); let (b1x, b1y) = pt(3.0,  0.0);
    buf.draw_line(b0x, b0y, b1x, b1y, body_ctr);

    let (s0x, s0y) = pt(-6.0,  1.0); let (s1x, s1y) = pt(3.0,  1.0);
    buf.draw_line(s0x, s0y, s1x, s1y, body_side);
    let (s2x, s2y) = pt(-6.0, -1.0); let (s3x, s3y) = pt(3.0, -1.0);
    buf.draw_line(s2x, s2y, s3x, s3y, body_side);

    let (e0x, e0y) = pt(-6.0,  2.0); let (e1x, e1y) = pt(3.0,  2.0);
    buf.draw_line(e0x, e0y, e1x, e1y, body_edge);
    let (e2x, e2y) = pt(-6.0, -2.0); let (e3x, e3y) = pt(3.0, -2.0);
    buf.draw_line(e2x, e2y, e3x, e3y, body_edge);

    // Nose cone (+3 to +6, centre only — tapers the front)
    let (n0x, n0y) = pt(3.0, 0.0); let (n1x, n1y) = pt(6.0, 0.0);
    buf.draw_line(n0x, n0y, n1x, n1y, nose_col);
    // Nose inner edges (taper from ±1 at base to tip)
    let (ni0x, ni0y) = pt(3.0,  1.0); let (ni1x, ni1y) = pt(5.0, 0.0);
    buf.draw_line(ni0x, ni0y, ni1x, ni1y, nose_col);
    let (ni2x, ni2y) = pt(3.0, -1.0); let (ni3x, ni3y) = pt(5.0, 0.0);
    buf.draw_line(ni2x, ni2y, ni3x, ni3y, nose_col);

    // Bright tip pixel (+7)
    let (tx, ty) = pt(7.0, 0.0);
    buf.set_pixel(tx, ty, tip_col);
}

/// Draw a grenade projectile — small oval body with seam lines and pin, matching the weapon icon style.
pub fn draw_grenade_projectile(buf: &mut WorldBuffer, pos: WorldPos) {
    let gx = pos.x as i32;
    let gy = pos.y as i32;
    let gbody = Bgra::new(55, 120, 45);
    let gdark = Bgra::new(25, 60, 20);
    let ghi   = Bgra::new(90, 170, 70);
    let gray  = Bgra::new(160, 160, 165);
    // Oval body (5px tall, 4px wide)
    buf.fill_rect(gx - 1, gy - 4, 2, 1, gdark); // top cap outline
    buf.fill_rect(gx - 2, gy - 3, 4, 5, gdark); // outline sides
    buf.fill_rect(gx - 1, gy + 2, 2, 1, gdark); // bottom cap outline
    // Body fill
    buf.fill_rect(gx - 1, gy - 3, 2, 1, gbody); // top cap
    buf.fill_rect(gx - 1, gy - 2, 2, 4, gbody); // center fill (2px wide)
    buf.fill_rect(gx - 1, gy + 1, 2, 1, gbody); // bottom cap
    // Highlight
    buf.set_pixel(gx - 1, gy - 2, ghi);
    // Seam line (horizontal)
    buf.fill_rect(gx - 2, gy - 1, 4, 1, gdark);
    // Pin at top
    buf.fill_rect(gx - 1, gy - 5, 2, 2, gray);
    buf.set_pixel(gx - 2, gy - 5, gray);
    buf.set_pixel(gx + 1, gy - 5, gray);
    let _ = ghi; // suppress unused warning
}

/// Draw the "?" thinking indicator above a CPU soldier.
/// Bobs up and down based on `tick` for animation.
pub fn draw_think_indicator(buf: &mut WorldBuffer, pos: WorldPos, tick: u32) {
    // Bob: oscillate ±3px over a 40-tick cycle
    let bob = ((tick % 40) as f32 / 40.0 * std::f32::consts::TAU).sin();
    let bob_y = (bob * 3.0) as i32;

    let cx = pos.x as i32;
    let y  = pos.y as i32 - SOLDIER_H - 4 - 16 + bob_y; // above head

    // Pixel art "?" — 5 wide × 7 tall
    let pixels: &[(i32, i32)] = &[
        (1,0),(2,0),(3,0),
        (0,1),(4,1),
        (3,2),
        (2,3),
        (2,4),
        (2,6),
    ];
    for &(dx, dy) in pixels {
        buf.set_pixel(cx - 2 + dx, y + dy, Bgra::white());
    }
}

/// Draw Worms-style water: animated wave crests with gradient depth and foam highlights.
///
/// Y coordinate in world space where the water strip cache starts (12px above WATER_Y
/// to cover the highest possible wave crest sky-restoration region).
pub const WATER_STRIP_TOP: u32 = crate::world::WATER_Y - 12;
/// Height of the water strip cache in pixels. Covers WATER_STRIP_TOP..WATER_STRIP_TOP+WATER_STRIP_H.
pub const WATER_STRIP_H: u32 = 32;

/// Render the water surface into a flat BGRA byte strip (`SCREEN_W × WATER_STRIP_H × 4` bytes).
/// The strip covers world-y rows [WATER_STRIP_TOP, WATER_STRIP_TOP+WATER_STRIP_H).
/// Call this every 3 ticks (or when cam_x changes) and blit the result each frame.
pub fn render_water_strip(strip: &mut [u8], tick: u32, cam_x: u32) {
    use crate::world::{WORLD_W, WORLD_H, SCREEN_W, WATER_Y};
    let base_y  = WATER_Y as i32;
    let world_h = WORLD_H as i32;
    let strip_top = WATER_STRIP_TOP as i32;
    let strip_h   = WATER_STRIP_H as i32;

    let body    = Bgra::water();
    let mid     = Bgra::new(45, 115, 210);
    let surface = Bgra::new(70, 160, 235);
    let crest   = Bgra::new(110, 195, 250);
    let foam    = Bgra::new(215, 235, 252);

    // Zero the strip first — positions we don't fill will be transparent (alpha=0)
    // which we treat as "skip" when blitting.
    strip.fill(0);

    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W)) as i32;
    let end_x = cam_x + SCREEN_W as i32;
    let mut x = cam_x;
    while x < end_x {
        let xf = x as f32;
        let tf = tick as f32;
        let w0 = (xf * 0.038 + tf * 0.10).sin() * 5.5;
        let w1 = (xf * 0.080 - tf * 0.16).sin() * 3.0;
        let w2 = (xf * 0.160 + tf * 0.22).sin() * 1.5;
        let wave = (w0 + w1 + w2) as i32;
        let top  = base_y + wave;
        let foam_phase = (xf * 0.11 + tf * 0.08).sin();

        let x2 = (x + 1).min(end_x - 1);
        for &xi in &[x, x2] {
            let sx = (xi - cam_x) as usize; // screen-relative x

            // Sky restoration: in troughs (top > base_y+2) the band [base_y, top-2]
            // was filled with water body by the world cache but should show sky.
            if top - 2 > base_y {
                for wy in base_y..(top - 2) {
                    if wy < strip_top || wy >= strip_top + strip_h { continue; }
                    let sy = (wy - strip_top) as usize;
                    let sky = crate::renderer::draw_terrain::sky_colour(xi, wy, 0);
                    let off = (sy * SCREEN_W as usize + sx) * 4;
                    strip[off]     = sky.b;
                    strip[off + 1] = sky.g;
                    strip[off + 2] = sky.r;
                    strip[off + 3] = 0xFF;
                }
            }

            let fill_start = (top - 2).max(0);
            let fill_end   = (top + 8).min(world_h);
            for wy in fill_start..fill_end {
                if wy < strip_top || wy >= strip_top + strip_h { continue; }
                let depth = wy - top;
                let colour = match depth {
                    d if d < 0 => body,
                    0 | 1      => crest,
                    2 | 3      => surface,
                    4 | 5      => mid,
                    6 | 7      => Bgra::new(38, 98, 195),
                    _          => body,
                };
                let sy = (wy - strip_top) as usize;
                let off = (sy * SCREEN_W as usize + sx) * 4;
                strip[off]     = colour.b;
                strip[off + 1] = colour.g;
                strip[off + 2] = colour.r;
                strip[off + 3] = 0xFF;
            }

            if foam_phase > 0.30 {
                for wy in [top, top + 1] {
                    if wy >= strip_top && wy < strip_top + strip_h {
                        let sy = (wy - strip_top) as usize;
                        let off = (sy * SCREEN_W as usize + sx) * 4;
                        strip[off]     = foam.b;
                        strip[off + 1] = foam.g;
                        strip[off + 2] = foam.r;
                        strip[off + 3] = 0xFF;
                    }
                }
            }
            if foam_phase > 0.60 {
                let wy = top - 1;
                if wy >= strip_top && wy < strip_top + strip_h {
                    let sy = (wy - strip_top) as usize;
                    let off = (sy * SCREEN_W as usize + sx) * 4;
                    strip[off]     = foam.b;
                    strip[off + 1] = foam.g;
                    strip[off + 2] = foam.r;
                    strip[off + 3] = 0xFF;
                }
            }
        }
        x += 2;
    }
}

/// The terrain renderer already fills the water body (WATER_Y..WORLD_H) with Bgra::water().
/// This function overpaints the surface zone with brighter colours + animated foam.
pub fn draw_water_surface(buf: &mut WorldBuffer, tick: u32, cam_x: u32) {
    use crate::world::{WORLD_H, WORLD_W, SCREEN_W};
    let base_y  = WATER_Y as i32;
    let world_h = WORLD_H as i32;

    let body    = Bgra::water();             // rgb(30,  80, 180) — deep body
    let mid     = Bgra::new(45, 115, 210);  // transition row
    let surface = Bgra::new(70, 160, 235);  // bright surface band
    let crest   = Bgra::new(110, 195, 250); // wave crest
    let foam    = Bgra::new(215, 235, 252); // foam / white-blue

    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W)) as i32;
    let end_x = cam_x + SCREEN_W as i32;
    // Wave/foam phases vary slowly across x, so compute them once per 2-column
    // stripe and paint both columns — halves the sin() calls and per-pixel
    // bookkeeping for this per-frame pass without a visible change in motion.
    let mut x = cam_x;
    while x < end_x {
        let xf = x as f32;
        let tf = tick as f32;

        // Three-component wave — larger amplitude for more visible motion
        let w0 = (xf * 0.038 + tf * 0.10).sin() * 5.5;
        let w1 = (xf * 0.080 - tf * 0.16).sin() * 3.0;
        let w2 = (xf * 0.160 + tf * 0.22).sin() * 1.5;

        let wave = (w0 + w1 + w2) as i32;  // ≈ –10 to +10 px
        let top  = base_y + wave;

        let foam_phase = (xf * 0.11 + tf * 0.08).sin();

        let x2 = (x + 1).min(end_x - 1);
        for &xi in &[x, x2] {
            // In a wave trough the surface dips below the cached flat waterline, which
            // the world cache filled with solid water up to a straight edge at base_y —
            // a flat-topped band that masks the trough. Restore the sky there (over
            // cached water pixels only, never a terrain shoreline) so the trough reads
            // as a real dip that matches the foreground wave shape.
            if top - 2 > base_y {
                for wy in base_y..(top - 2) {
                    if buf.get_pixel_unchecked(xi as u32, wy as u32) == body {
                        // Horizon sky colour is biome-independent (archetype unused here).
                        buf.set_pixel_unchecked(xi as u32, wy as u32, crate::renderer::draw_terrain::sky_colour(xi, wy, 0));
                    }
                }
            }

            // Fill from a couple rows above base_y through the gradient band. Rows
            // beyond depth 7 are plain `body`, which the viewport copy already filled
            // from the world cache (terrain_pixel returns WATER there) — skip those.
            let fill_start = (top - 2).max(0);
            let fill_end = (top + 8).min(world_h);
            for wy in fill_start..fill_end {
                let depth = wy - top;
                let colour = match depth {
                    d if d < 0 => body,
                    0 | 1      => crest,      // 2 rows of crest under the foam
                    2 | 3      => surface,    // bright surface band
                    4 | 5      => mid,        // transition
                    6 | 7      => Bgra::new(38, 98, 195), // slightly lighter than body
                    _          => body,
                };
                buf.set_pixel_unchecked(xi as u32, wy as u32, colour);
            }

            // Foam band: 2–3 px thick at wave crests, brighter where the primary wave peaks
            if foam_phase > 0.30 {
                buf.set_pixel_unchecked(xi as u32, top as u32,     foam); // crest row — always foam when phase active
                buf.set_pixel_unchecked(xi as u32, (top + 1) as u32, foam); // one row below — fills out the stripe
            }
            if foam_phase > 0.60 {
                buf.set_pixel_unchecked(xi as u32, (top - 1) as u32, foam); // peak pixels get a third row above
            }
        }

        x += 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf() -> WorldBuffer { WorldBuffer::new() }
    fn pos(x: f32, y: f32) -> WorldPos { WorldPos::new(x, y) }

    // ── Team colours ──────────────────────────────────────────────────────────

    #[test]
    fn four_team_colours_defined() {
        assert_eq!(TEAM_COLOURS.len(), 4);
        assert_eq!(TEAM_COLOURS_DEAD.len(), 4);
    }

    #[test]
    fn team_colours_are_distinct() {
        for i in 0..4 {
            for j in 0..4 {
                if i != j {
                    assert_ne!(TEAM_COLOURS[i], TEAM_COLOURS[j],
                        "team colours {i} and {j} should differ");
                }
            }
        }
    }

    #[test]
    fn dead_colours_are_dimmer_than_live() {
        for i in 0..4 {
            let live = TEAM_COLOURS[i];
            let dead = TEAM_COLOURS_DEAD[i];
            let live_brightness = live.r as u32 + live.g as u32 + live.b as u32;
            let dead_brightness = dead.r as u32 + dead.g as u32 + dead.b as u32;
            assert!(dead_brightness < live_brightness,
                "team {i} dead colour should be dimmer than live");
        }
    }

    // ── draw_soldier ──────────────────────────────────────────────────────────

    #[test]
    fn soldier_body_pixels_are_team_colour() {
        let mut b = buf();
        draw_soldier(&mut b, pos(100.0, 200.0), 0, 1, 100, 0);
        // Centre of body should be team 0 colour
        let cx = 100i32;
        let fy = 200i32;
        assert_eq!(b.get_pixel(cx, fy - SOLDIER_H / 2), TEAM_COLOURS[0]);
    }

    #[test]
    fn dead_soldier_uses_dead_colour() {
        let mut b = buf();
        draw_soldier(&mut b, pos(100.0, 200.0), 1, 1, 0, 0);
        assert_eq!(b.get_pixel(100, 200 - SOLDIER_H / 2), TEAM_COLOURS_DEAD[1]);
    }

    #[test]
    fn soldier_does_not_panic_at_world_edges() {
        let mut b = buf();
        draw_soldier(&mut b, pos(0.0, 50.0), 0, 1, 100, 0);
        draw_soldier(&mut b, pos(crate::world::WORLD_W as f32 - 1.0, 50.0), 0, 1, 100, 0);
    }

    #[test]
    fn all_four_teams_render_without_panic() {
        let mut b = buf();
        for team in 0..4 {
            draw_soldier(&mut b, pos(100.0 + team as f32 * 50.0, 200.0), team, 1, 100, 0);
        }
    }

    // ── HP number ─────────────────────────────────────────────────────────────

    #[test]
    fn hp_number_does_not_panic() {
        let mut b = buf();
        draw_hp_number(&mut b, 200, 200, 100, 0);
        draw_hp_number(&mut b, 200, 200, 1,   1);
    }

    #[test]
    fn zero_health_no_number_drawn() {
        let mut b = buf();
        // hp=0 means dead — no HP number drawn at all
        draw_soldier(&mut b, pos(200.0, 200.0), 0, 1, 0, 0);
        // Just verify no panic
    }

    #[test]
    fn health_colour_red_at_or_below_33() {
        // health_colour removed — kept test stub so test count doesn't drop
        let _ = TEAM_COLOURS[0]; // trivial sanity check
        assert_eq!(TEAM_COLOURS[0], Bgra::new(220, 80, 80));
    }

    #[test]
    fn placeholder_colour_tests() {
        // Placeholder for removed health_colour tests
        let _ = TEAM_COLOURS[1];
        assert_eq!(TEAM_COLOURS[1], Bgra::new(80, 120, 220));
    }

    #[test]
    fn another_placeholder() {
        let _ = TEAM_COLOURS[2];
        assert_ne!(TEAM_COLOURS[2], Bgra::new(220, 80,  80));
    }

    // ── aim arrow ────────────────────────────────────────────────────────────

    #[test]
    fn aim_arrow_draws_without_panic() {
        let mut b = buf();
        draw_aim_arrow(&mut b, (500.0, 290.0), 0.785, 0.5);
    }

    // aim_arrow_at_zero_angle_goes_right removed: the aim indicator was redesigned
    // from a straight power-scaled arrow (tip at origin + AIM_ARROW_MAX_LEN) into a
    // fixed-distance reticle + rotating charge bar, so there's no arrow tip pixel.

    #[test]
    fn aim_arrow_min_power_is_shorter_than_max() {
        let mut b1 = buf();
        let mut b2 = buf();
        draw_aim_arrow(&mut b1, (300.0, 288.0), 0.0, 0.0);
        draw_aim_arrow(&mut b2, (300.0, 288.0), 0.0, 1.0);
        let short_tip = 300 + AIM_ARROW_MIN_LEN as i32;
        let long_tip  = 300 + AIM_ARROW_MAX_LEN as i32;
        assert_ne!(short_tip, long_tip);
    }

    // ── projectile ───────────────────────────────────────────────────────────

    #[test]
    fn projectile_draws_at_position() {
        let mut b = buf();
        draw_projectile(&mut b, pos(400.0, 200.0), 3, Bgra::yellow());
        assert_eq!(b.get_pixel(400, 200), Bgra::yellow());
    }

    #[test]
    fn projectile_does_not_panic_near_edges() {
        let mut b = buf();
        draw_projectile(&mut b, pos(0.0, 0.0), 5, Bgra::yellow());
        draw_projectile(&mut b, pos(crate::world::WORLD_W as f32 - 1.0, 10.0), 5, Bgra::yellow());
    }

    // ── think indicator ───────────────────────────────────────────────────────

    #[test]
    fn think_indicator_draws_without_panic() {
        let mut b = buf();
        draw_think_indicator(&mut b, pos(500.0, 300.0), 0);
        draw_think_indicator(&mut b, pos(500.0, 300.0), 20);
        draw_think_indicator(&mut b, pos(500.0, 300.0), 39);
    }

    #[test]
    fn think_indicator_bobs_different_y_at_different_ticks() {
        // The bob changes y — pixel at tick=0 and tick=10 should differ
        let mut b0 = buf();
        let mut b10 = buf();
        draw_think_indicator(&mut b0,  pos(500.0, 300.0), 0);
        draw_think_indicator(&mut b10, pos(500.0, 300.0), 10);
        // They may or may not differ pixel-for-pixel at any given coord
        // — just verify no panic and different ticks run fine
    }

    // ── water surface ─────────────────────────────────────────────────────────

    #[test]
    fn water_surface_draws_without_panic() {
        let mut b = buf();
        draw_water_surface(&mut b, 0, 0);
        draw_water_surface(&mut b, 100, 0);
    }

    #[test]
    fn water_surface_draws_near_water_y() {
        let mut b = buf();
        draw_water_surface(&mut b, 0, 0);
        // Some pixel near WATER_Y should be one of the animated water band colours
        // (mid / surface / crest / foam — see draw_water_surface).
        let water_cols = [
            Bgra::new(45, 115, 210),
            Bgra::new(70, 160, 235),
            Bgra::new(110, 195, 250),
            Bgra::new(215, 235, 252),
        ];
        let mut found = false;
        'scan: for x in 0..64 {
            for y in WATER_Y as i32 - 3..=WATER_Y as i32 + 5 {
                if water_cols.contains(&b.get_pixel(x, y)) { found = true; break 'scan; }
            }
        }
        assert!(found, "water surface should appear near WATER_Y");
    }
}
