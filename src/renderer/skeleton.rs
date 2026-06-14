use crate::world::WorldPos;
use super::buffer::WorldBuffer;
use super::fb::Bgra;
use super::draw_sprites::{TEAM_COLOURS, TEAM_COLOURS_DEAD, draw_hp_number_lifted};

// ── Bone indices ─────────────────────────────────────────────────────────────

const TORSO: usize = 0;
const HEAD:  usize = 1;
const ARM_R: usize = 2;
const ARM_L: usize = 3;
const LEG_R: usize = 4;
const LEG_L: usize = 5;
const N_BONES: usize = 6;

struct Bone {
    parent: Option<usize>,
    length: f32,
    angle:  f32, // local angle relative to parent (or world if no parent)
}

/// Animation state passed from the game layer to the renderer.
pub enum SoldierAnim {
    Idle,
    Walking { tick: u32 },
    Airborne { vel_x: f32, vel_y: f32, airtime: u32, spinning: bool },
    Dead,
}

// ── Bone math ────────────────────────────────────────────────────────────────

fn rot(x: f32, y: f32, a: f32) -> (f32, f32) {
    let (s, c) = a.sin_cos();
    (x * c - y * s, x * s + y * c)
}

fn smoothstep(t: f32) -> f32 { t * t * (3.0 - 2.0 * t) }

/// Compute world (x, y) of each bone endpoint given the hip root position.
fn compute_positions(root: (f32, f32), bones: &[Bone; N_BONES]) -> [(f32, f32); N_BONES] {
    let mut origins: [(f32, f32); N_BONES] = [root; N_BONES];
    let mut world_angles = [0f32; N_BONES];
    let mut ends = [(0f32, 0f32); N_BONES];

    for i in 0..N_BONES {
        let parent_origin = bones[i].parent.map_or(root, |p| ends[p]);
        let parent_angle  = bones[i].parent.map_or(0.0,  |p| world_angles[p]);
        origins[i] = parent_origin;

        let world_angle = parent_angle + bones[i].angle;
        world_angles[i] = world_angle;

        // Bone points upward in local space (negative Y = up on screen)
        let (dx, dy) = rot(0.0, -bones[i].length, world_angle);
        ends[i] = (parent_origin.0 + dx, parent_origin.1 + dy);
    }
    ends
}

// ── Animation functions ───────────────────────────────────────────────────────

fn default_bones() -> [Bone; N_BONES] {
    [
        Bone { parent: None,        length: 13.0, angle: 0.0 },                        // TORSO: up
        Bone { parent: Some(TORSO), length: 6.0,  angle: 0.0 },                        // HEAD: up from shoulder
        Bone { parent: Some(TORSO), length: 9.0,  angle: 0.0 },                        // ARM_R
        Bone { parent: Some(TORSO), length: 9.0,  angle: 0.0 },                        // ARM_L
        Bone { parent: None,        length: 11.0, angle: std::f32::consts::PI },        // LEG_R: down from hip
        Bone { parent: None,        length: 11.0, angle: std::f32::consts::PI },        // LEG_L: down from hip
    ]
}

fn pose_idle(bones: &mut [Bone; N_BONES], t: f32) {
    let breath = (t * 1.8).sin() * 0.04;
    bones[TORSO].angle = breath;
    bones[HEAD].angle  = -breath * 0.5;
    bones[ARM_R].angle = 0.6;  // arms hang at sides
    bones[ARM_L].angle = -0.6;
    bones[LEG_R].angle = std::f32::consts::PI - 0.05;
    bones[LEG_L].angle = std::f32::consts::PI + 0.05;
}

fn walk_swing_r(tick: u32) -> f32 {
    const STRIDE: f32 = 20.0;
    let phase = (tick as f32 % STRIDE) / STRIDE;
    let t4 = phase * 4.0;
    let frac = smoothstep(t4.fract());
    match t4.floor() as u32 % 4 {
        0 => 1.0 - frac,
        1 => -frac,
        2 => -1.0 + frac,
        _ => frac,
    }
}

fn pose_walk(bones: &mut [Bone; N_BONES], tick: u32, facing: f32) {
    use std::f32::consts::PI;
    const LEG_AMP: f32 = 0.6;   // ±34° from PI at contact
    const ARM_AMP: f32 = 0.26;  // ±15°

    let swing_r = walk_swing_r(tick);
    let bob = swing_r * swing_r; // 1 at contact, 0 at passing

    bones[TORSO].angle = swing_r * 0.06;
    bones[HEAD].angle  = (0.5 - bob) * 0.08; // forward tilt at passing, back at contact
    bones[LEG_R].angle = PI + swing_r * LEG_AMP;
    bones[LEG_L].angle = PI - swing_r * LEG_AMP;
    bones[ARM_R].angle = -swing_r * ARM_AMP * facing + 0.4;
    bones[ARM_L].angle =  swing_r * ARM_AMP * facing - 0.4;
}

fn pose_airborne(bones: &mut [Bone; N_BONES], vel_x: f32, vel_y: f32) {
    let lean = (vel_x * 0.025).clamp(-0.5, 0.5);
    let tuck = (-vel_y * 0.03).clamp(-0.4, 0.25);
    bones[TORSO].angle = lean;
    bones[HEAD].angle  = -lean * 0.3;
    bones[LEG_R].angle = std::f32::consts::PI + tuck + 0.15;
    bones[LEG_L].angle = std::f32::consts::PI + tuck - 0.15;
    bones[ARM_R].angle = lean * 0.5 + 0.4;
    bones[ARM_L].angle = lean * 0.5 - 0.4;
}

fn pose_spin(bones: &mut [Bone; N_BONES], airtime: u32, facing: f32) {
    use std::f32::consts::{PI, TAU};
    // Negative facing direction: head tilts backward (away from facing) over legs
    let angle = -(facing) * airtime as f32 / 18.0 * TAU;
    bones[TORSO].angle = angle;
    bones[HEAD].angle  = 0.0;    // local to TORSO → tumbles with it
    bones[ARM_R].angle =  0.35;  // tucked relative to torso
    bones[ARM_L].angle = -0.35;
    bones[LEG_R].angle = PI + angle + 0.12;
    bones[LEG_L].angle = PI + angle - 0.12;
}

fn pose_dead(bones: &mut [Bone; N_BONES], facing: f32) {
    // Flop sideways in facing direction
    let flop = std::f32::consts::FRAC_PI_2 * facing;
    bones[TORSO].angle = flop;
    bones[HEAD].angle  = std::f32::consts::FRAC_PI_4 * facing;
    bones[LEG_R].angle = std::f32::consts::PI + 0.9;
    bones[LEG_L].angle = std::f32::consts::PI + 0.3;
    bones[ARM_R].angle = -1.1;
    bones[ARM_L].angle =  0.4;
}

// ── Rendering helpers ─────────────────────────────────────────────────────────

fn thick_line(buf: &mut WorldBuffer, ax: f32, ay: f32, bx: f32, by: f32, col: Bgra, w: i32) {
    let dx = bx - ax;
    let dy = by - ay;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let px = ((-dy / len).round()) as i32;
    let py = (( dx / len).round()) as i32;
    let half = w / 2;
    for o in -half..=half {
        buf.draw_line(ax as i32 + px*o, ay as i32 + py*o,
                      bx as i32 + px*o, by as i32 + py*o, col);
    }
}

// ── Cosmetic color lookups ────────────────────────────────────────────────────

fn uniform_color(id: u8) -> Bgra {
    match id {
        1 => Bgra::new( 60, 100,  50), // Camo Green
        2 => Bgra::new(190, 155,  90), // Desert Tan
        3 => Bgra::new( 30,  30,  35), // Midnight Black
        4 => Bgra::new(230, 230, 235), // Snow White
        5 => Bgra::new( 30,  40, 120), // Navy
        6 => Bgra::new(200, 120, 160), // Pink Camo
        7 => Bgra::new(200, 165,  40), // Gold Plate
        _ => Bgra::new(  0,   0,   0), // fallback (caller should pass team_col for id=0)
    }
}

fn boot_color(id: u8) -> Bgra {
    match id {
        1 => Bgra::new(180,  40,  40), // Red
        2 => Bgra::new(220, 215, 205), // White
        3 => Bgra::new(190, 155,  30), // Gold
        4 => Bgra::new( 50,  80,  40), // Combat Green
        5 => Bgra::new( 30,  80, 220), // Electric Blue
        _ => Bgra::new( 35,  30,  22), // Default dark brown
    }
}

// ── Hat drawing ───────────────────────────────────────────────────────────────

fn draw_hat(buf: &mut WorldBuffer, cx: i32, cy: i32, hat_id: u8) {
    let dark = Bgra::new(22, 14, 6);
    // Hats are drawn at 2× scale so the shape is readable in-game. All shape
    // coordinates below are in the original logical (small) units; `rect`/`dot`
    // multiply position AND size by S, drawing chunky scaled pixels.
    const S: i32 = 2;
    // Anchor: keep the hat base on the head top (logical dy=-5 → head_cy-5) after
    // scaling, so the bigger hat still sits on the head instead of floating above it.
    const AY: i32 = 8;
    let rect = |buf: &mut WorldBuffer, ldx: i32, ldy: i32, lw: i32, lh: i32, col: Bgra| {
        buf.fill_rect(cx + ldx * S, cy + ldy * S + AY, (lw * S) as u32, (lh * S) as u32, col);
    };
    let dot = |buf: &mut WorldBuffer, ldx: i32, ldy: i32, col: Bgra| {
        buf.fill_rect(cx + ldx * S, cy + ldy * S + AY, S as u32, S as u32, col);
    };
    match hat_id {
        1 => { // Top Hat
            let col = Bgra::new(30, 20, 15);
            rect(buf, -3, -10, 7, 5, col);   // cylinder
            rect(buf, -4,  -5, 9, 2, col);   // brim
            rect(buf, -4,  -5, 9, 1, dark);  // brim shadow
        }
        2 => { // Propeller Hat
            let col = Bgra::new(60, 80, 200);
            rect(buf, -2, -9, 5, 4, col);    // bowl
            rect(buf, -3, -5, 7, 1, dark);   // rim
            rect(buf, -4, -9, 4, 1, Bgra::new(220, 40, 40)); // blade L
            rect(buf,  1, -9, 4, 1, Bgra::new(220, 40, 40)); // blade R
            dot(buf, 0, -10, dark);          // pin
        }
        3 => { // Flower
            let petal = Bgra::new(240, 100, 160);
            let center = Bgra::new(255, 220, 40);
            dot(buf,  0,  -8, center);
            dot(buf, -1,  -9, petal);
            dot(buf,  1,  -9, petal);
            dot(buf,  0, -10, petal);
            dot(buf, -2,  -8, petal);
            dot(buf,  2,  -8, petal);
        }
        4 => { // Crown
            let col = Bgra::new(220, 170, 0);
            rect(buf, -3, -9, 7, 3, col);    // base band
            dot(buf, -3, -10, col);          // left spike
            dot(buf,  0, -11, col);          // center spike
            dot(buf,  3, -10, col);          // right spike
        }
        5 => { // Fez
            let col = Bgra::new(180, 20, 20);
            rect(buf, -2, -10, 5, 5, col);
            rect(buf, -3,  -5, 7, 1, dark);  // brim line
            dot(buf, 0, -11, Bgra::new(200, 190, 150)); // tassel
            dot(buf, 1, -10, Bgra::new(200, 190, 150));
        }
        6 => { // Beret
            let col = Bgra::new(40, 90, 40);
            rect(buf, -4, -8, 8, 3, col);    // flat body
            dot(buf, -4, -7, dark);          // left edge
            dot(buf,  3, -7, dark);          // right edge
        }
        7 => { // Party Hat
            let col = Bgra::new(220, 60, 200);
            dot(buf, 0, -11, Bgra::new(255, 220, 50)); // tip star
            rect(buf, -1, -10, 3, 2, col);
            rect(buf, -2,  -8, 5, 2, col);
            rect(buf, -3,  -6, 7, 1, dark);
        }
        8 => { // Halo
            let col = Bgra::new(255, 220, 50);
            for dx in -3i32..=3 {
                let dy = if dx.abs() <= 1 { -12 } else { -11 };
                dot(buf, dx, dy, col);
            }
        }
        9 => { // Devil Horns
            let col = Bgra::new(200, 30, 30);
            dot(buf, -3,  -9, col);
            dot(buf, -4, -10, col);
            dot(buf, -3, -11, col);
            dot(buf,  3,  -9, col);
            dot(buf,  4, -10, col);
            dot(buf,  3, -11, col);
        }
        10 => { // Gold Crown (premium)
            let col = Bgra::new(255, 200, 0);
            let gem = Bgra::new(100, 180, 255);
            rect(buf, -3, -9, 7, 3, col);
            dot(buf, -3, -10, col);
            dot(buf,  0, -11, col);
            dot(buf,  3, -10, col);
            dot(buf,  0,  -9, gem); // center gem
        }
        11 => { // Laurel Wreath
            let col = Bgra::new(60, 160, 50);
            for (dx, dy) in [(-4,-8),(-3,-9),(-2,-9),(-1,-10),(0,-10),(1,-10),(2,-9),(3,-9),(4,-8)] {
                dot(buf, dx, dy, col);
            }
            for (dx, dy) in [(-3,-7),(-2,-8),(2,-8),(3,-7)] {
                dot(buf, dx, dy, col);
            }
        }
        12 => { // Blue Party Hat
            let col = Bgra::new(50, 120, 230);
            dot(buf, 0, -11, Bgra::new(255, 255, 255)); // white tip star
            rect(buf, -1, -10, 3, 2, col);
            rect(buf, -2,  -8, 5, 2, col);
            rect(buf, -3,  -6, 7, 1, dark);
        }
        _ => {}
    }
}

// ── Gun style drawing ─────────────────────────────────────────────────────────

/// Draw the gun and return the barrel tip (x, y) for the charge meter origin.
/// `origin` is the forward arm endpoint. `disp` is the display angle (already
/// adjusted for facing direction). All styles use fwd + perp offset math so
/// they rotate correctly with the aim angle.
fn draw_gun_style(buf: &mut WorldBuffer, origin: (f32, f32), disp: f32, gun_style_id: u8) -> (f32, f32) {
    let dark = Bgra::new(22, 14, 6);
    let fwd_x =  disp.cos();
    let fwd_y = -disp.sin();
    let prp_x =  disp.sin();  // perpendicular (rotated 90°)
    let prp_y =  disp.cos();

    // Helper: pixel at origin + fwd*t + perp*p
    let px = |t: f32, p: f32| -> (i32, i32) {
        ((origin.0 + fwd_x * t + prp_x * p).round() as i32,
         (origin.1 + fwd_y * t + prp_y * p).round() as i32)
    };

    match gun_style_id {
        0 | _ if gun_style_id == 0 => {
            // Default: 3-line barrel, gray
            let gun = Bgra::new(72, 72, 78);
            let tip = px(9.0, 0.0);
            for t in 0..=9i32 {
                let (x, y) = px(t as f32, 0.0);
                let (xd, _yd) = px(t as f32, 0.0);
                buf.set_pixel(x,  y - 1, dark);
                buf.set_pixel(xd, y,     gun);
                buf.set_pixel(x,  y + 1, dark);
            }
            (tip.0 as f32, tip.1 as f32)
        }
        1 => {
            // Pistol: short thick barrel, wider receiver block
            let gun = Bgra::new(80, 80, 90);
            for t in 0..=6i32 {
                for p in -1i32..=1 {
                    let c = if p == 0 { gun } else { dark };
                    let (x, y) = px(t as f32, p as f32);
                    buf.set_pixel(x, y, c);
                }
            }
            // Receiver block at base
            for p in -2i32..=2 { let (x,y) = px(1.0, p as f32); buf.set_pixel(x, y, dark); }
            for p in -1i32..=1 { let (x,y) = px(1.0, p as f32); buf.set_pixel(x, y, gun); }
            let tip = px(6.0, 0.0);
            (tip.0 as f32, tip.1 as f32)
        }
        2 => {
            // Shotgun: double barrel (two parallel lines)
            let gun = Bgra::new(70, 55, 40);
            for t in 0..=10i32 {
                let tf = t as f32;
                for &p in &[-1.5f32, 1.5] {
                    let (x, y) = px(tf, p);
                    buf.set_pixel(x, y, gun);
                    let (xd, yd) = px(tf, if p < 0.0 { p - 1.0 } else { p + 1.0 });
                    buf.set_pixel(xd, yd, dark);
                }
            }
            let tip = px(10.0, 0.0);
            (tip.0 as f32, tip.1 as f32)
        }
        3 => {
            // Sniper: long thin barrel + scope bump
            let gun = Bgra::new(50, 50, 60);
            for t in 0..=14i32 {
                let (x, y) = px(t as f32, 0.0);
                buf.set_pixel(x, y - 1, dark);
                buf.set_pixel(x, y,     gun);
            }
            // Scope bump at mid-barrel
            for p in -1i32..=1 { let (x,y) = px(6.0, p as f32); buf.set_pixel(x, y, Bgra::new(90,90,100)); }
            for p in -1i32..=1 { let (x,y) = px(7.0, p as f32); buf.set_pixel(x, y, Bgra::new(90,90,100)); }
            let tip = px(14.0, 0.0);
            (tip.0 as f32, tip.1 as f32)
        }
        4 => {
            // Minigun: 3 barrel stubs around center axis
            let gun = Bgra::new(80, 80, 85);
            for t in 0..=8i32 {
                let tf = t as f32;
                for &p in &[-2.0f32, 0.0, 2.0] {
                    let (x, y) = px(tf, p);
                    buf.set_pixel(x, y, gun);
                }
            }
            // Housing block at base
            for p in -3i32..=3 { let (x,y) = px(1.0, p as f32); buf.set_pixel(x, y, dark); }
            let tip = px(8.0, 0.0);
            (tip.0 as f32, tip.1 as f32)
        }
        5 => {
            // Cannon: very wide short barrel
            let gun = Bgra::new(55, 55, 60);
            for t in 0..=6i32 {
                for p in -2i32..=2 {
                    let c = if p.abs() == 2 { dark } else { gun };
                    let (x, y) = px(t as f32, p as f32);
                    buf.set_pixel(x, y, c);
                }
            }
            let tip = px(6.0, 0.0);
            (tip.0 as f32, tip.1 as f32)
        }
        6 => {
            // Laser: thin single line, glowing cyan tip
            let beam = Bgra::new(0, 220, 255);
            let glow = Bgra::new(180, 245, 255);
            for t in 0..=11i32 {
                let c = if t >= 9 { glow } else { beam };
                let (x, y) = px(t as f32, 0.0);
                buf.set_pixel(x, y, c);
            }
            let tip = px(11.0, 0.0);
            (tip.0 as f32, tip.1 as f32)
        }
        7 => {
            // Golden Gun: 3-line barrel in gold
            let gold    = Bgra::new(210, 170, 20);
            let dk_gold = Bgra::new(130, 100,  5);
            for t in 0..=10i32 {
                let (x, y) = px(t as f32, 0.0);
                buf.set_pixel(x, y - 1, dk_gold);
                buf.set_pixel(x, y,     gold);
                buf.set_pixel(x, y + 1, dk_gold);
            }
            let tip = px(10.0, 0.0);
            (tip.0 as f32, tip.1 as f32)
        }
        8 => {
            // Throwable: small oval held in hand (grenade/bomb shape)
            let body = Bgra::new(55, 120, 45);
            let hilit = Bgra::new(90, 160, 70);
            for p in -2i32..=2 {
                for t in 0i32..=4 {
                    let c = if p.abs() == 2 || t == 0 || t == 4 { dark } else { body };
                    let (x, y) = px(t as f32, p as f32);
                    buf.set_pixel(x, y, c);
                }
            }
            // Highlight
            let (hx, hy) = px(1.0, -1.0);
            buf.set_pixel(hx, hy, hilit);
            // Pin
            let (pinx, piny) = px(1.0, -3.0);
            buf.set_pixel(pinx, piny, Bgra::new(160, 160, 165));
            let tip = px(4.0, 0.0);
            (tip.0 as f32, tip.1 as f32)
        }
        _ => {
            let tip = px(9.0, 0.0);
            (tip.0 as f32, tip.1 as f32)
        }
    }
}

// ── Public draw function ──────────────────────────────────────────────────────

/// Draw a soldier using procedural skeletal animation.
pub fn draw_soldier_skeletal(
    buf:              &mut WorldBuffer,
    pos:              WorldPos,
    team:             usize,
    facing:           i8,
    hp:               u8,
    anim:             &SoldierAnim,
    aim_angle:        Option<f32>,
    show_hp:          bool,
    hat_id:           u8,
    uniform_color_id: u8,
    boot_color_id:    u8,
    gun_style_id:     u8,
) -> Option<(f32, f32)> {
    let team_col = if hp == 0 { TEAM_COLOURS_DEAD[team.min(3)] } else { TEAM_COLOURS[team.min(3)] };
    // body_col: uniform override for torso/arms/legs; helmet cap always keeps team_col
    let body_col = if uniform_color_id == 0 { team_col } else { uniform_color(uniform_color_id) };
    let skin_col = Bgra::new(218, 178, 140);
    let dark_col = Bgra::new(22,  14,  6);
    let boot_col = boot_color(boot_color_id);

    let f = facing as f32; // +1 right, -1 left

    // Hip = root; Walking gets lateral sway + body rise at passing phase
    let root = match anim {
        SoldierAnim::Walking { tick } => {
            let sr = walk_swing_r(*tick);
            let bob = sr * sr; // 1 at contact, 0 at passing
            let rise = (1.0 - bob) * 2.0; // body rises 2px at passing, sits at contact
            (pos.x + sr * 3.0 * f, pos.y - 11.0 - rise)
        }
        _ => (pos.x, pos.y - 11.0),
    };

    let mut bones = default_bones();

    // Select animation
    match anim {
        SoldierAnim::Idle =>
            pose_idle(&mut bones, pos.x * 0.0 + 0.0), // will use game tick passed as part of pos hack — use 0 for now; caller sets via Walking{tick}
        SoldierAnim::Walking { tick } =>
            pose_walk(&mut bones, *tick, f),
        SoldierAnim::Airborne { vel_x: _, vel_y: _, airtime, spinning: true } =>
            pose_spin(&mut bones, *airtime, f),
        SoldierAnim::Airborne { vel_x, vel_y, .. } =>
            pose_airborne(&mut bones, *vel_x, *vel_y),
        SoldierAnim::Dead =>
            pose_dead(&mut bones, f),
    }

    // Override arm angle to track aim when aiming
    if let Some(aim) = aim_angle {
        // aim_angle is world-space. Torso world angle is bones[TORSO].angle (no parent).
        // ARM_R/ARM_L world angle = torso_angle + arm_local_angle.
        // We want arm endpoint to point in aim direction from shoulder.
        // Arm points up (-π/2) when local angle = 0 and torso angle = 0.
        // World angle of bone = torso.angle + arm.angle.
        // We want world angle such that the arm endpoint is in the aim direction.
        //   aim_angle is from horizontal right = 0. Screen Y is inverted so:
        //   bone world_angle = -(aim_world) - π/2  (because bones point with rot(0,-len,angle))
        let torso_world = bones[TORSO].angle;
        // Weapon is on the facing side
        let aim_disp = if f >= 0.0 { aim } else { std::f32::consts::PI - aim };
        // The bone endpoint direction: rot(0, -len, world_angle).
        // We want that direction to be (cos(aim_disp), -sin(aim_disp)) (screen coords).
        // rot(0,-1,wa) = (sin(wa), -cos(wa)). So sin(wa) = cos(aim_disp), -cos(wa) = -sin(aim_disp).
        // wa = π/2 - aim_disp + n*2π. Local = wa - torso_world.
        let arm_world = std::f32::consts::FRAC_PI_2 - aim_disp;
        let arm_local = arm_world - torso_world;
        if f >= 0.0 { bones[ARM_R].angle = arm_local; }
        else        { bones[ARM_L].angle = arm_local; }
    }

    let ends = compute_positions(root, &bones);

    let hip       = root;
    let shoulder  = ends[TORSO];
    let head_top  = ends[HEAD];
    let arm_r_end = ends[ARM_R];
    let arm_l_end = ends[ARM_L];
    let leg_r_end = ends[LEG_R];
    let leg_l_end = ends[LEG_L];

    // Head center = midpoint between shoulder and head_top, or just shoulder + a bit
    let head_cx = shoulder.0 as i32;
    let head_cy = (shoulder.1 - 4.0) as i32;

    // Arms attach at 70% up the torso (chest level, below head)
    let arm_orig = (
        hip.0 + (shoulder.0 - hip.0) * 0.70,
        hip.1 + (shoulder.1 - hip.1) * 0.70,
    );
    let shift = (arm_orig.0 - shoulder.0, arm_orig.1 - shoulder.1);
    let arm_r_vis = (arm_r_end.0 + shift.0, arm_r_end.1 + shift.1);
    let arm_l_vis = (arm_l_end.0 + shift.0, arm_l_end.1 + shift.1);

    // ── Knee positions: midpoint offset forward in facing dir (back leg bends more) ──
    let walk_sr = match anim {
        SoldierAnim::Walking { tick } => walk_swing_r(*tick),
        _ => 0.0,
    };
    // bend_r: right leg is behind body (swing < 0) → max bend; front → 0
    let bend_r = match anim {
        SoldierAnim::Dead => 0.0,
        _                 => (1.0 - walk_sr) * 0.5 * 3.5,
    };
    // bend_l: left leg is behind body (swing > 0) → max bend; front → 0
    let bend_l = match anim {
        SoldierAnim::Dead => 0.0,
        _                 => (1.0 + walk_sr) * 0.5 * 3.5,
    };
    let knee_r = { let m = ((hip.0+leg_r_end.0)*0.5, (hip.1+leg_r_end.1)*0.5); (m.0+f*bend_r, m.1) };
    let knee_l = { let m = ((hip.0+leg_l_end.0)*0.5, (hip.1+leg_l_end.1)*0.5); (m.0+f*bend_l, m.1) };

    // Front leg = right when walk_sr >= 0, left otherwise
    let (back_knee, back_foot, front_knee, front_foot) = if walk_sr >= 0.0 {
        (knee_l, leg_l_end, knee_r, leg_r_end)
    } else {
        (knee_r, leg_r_end, knee_l, leg_l_end)
    };

    // ── Back leg (drawn before body for correct depth) ────────────────────────
    thick_line(buf, hip.0, hip.1, back_knee.0, back_knee.1, dark_col, 7);
    thick_line(buf, hip.0, hip.1, back_knee.0, back_knee.1, body_col, 5);
    thick_line(buf, back_knee.0, back_knee.1, back_foot.0, back_foot.1, dark_col, 7);
    thick_line(buf, back_knee.0, back_knee.1, back_foot.0, back_foot.1, body_col, 5);
    buf.fill_rect(back_foot.0 as i32 - 1, back_foot.1 as i32 - 1, 4, 3, boot_col);

    // ── Back arm ──────────────────────────────────────────────────────────────
    let (back_arm, fwd_arm) = if f >= 0.0 { (arm_l_vis, arm_r_vis) } else { (arm_r_vis, arm_l_vis) };
    thick_line(buf, arm_orig.0, arm_orig.1, back_arm.0, back_arm.1, dark_col, 5);
    thick_line(buf, arm_orig.0, arm_orig.1, back_arm.0, back_arm.1, body_col, 3);

    // ── Torso ─────────────────────────────────────────────────────────────────
    thick_line(buf, hip.0, hip.1, shoulder.0, shoulder.1, dark_col, 7);
    thick_line(buf, hip.0, hip.1, shoulder.0, shoulder.1, body_col, 5);
    // Belt stripe
    let belt = ((hip.0 + shoulder.0) / 2.0, (hip.1 + shoulder.1) / 2.0);
    let tdx = shoulder.0 - hip.0;
    let tdy = shoulder.1 - hip.1;
    let tlen = (tdx * tdx + tdy * tdy).sqrt().max(0.001);
    let tpx = (-tdy / tlen) * 3.0;
    let tpy = ( tdx / tlen) * 3.0;
    buf.draw_line((belt.0 - tpx) as i32, (belt.1 - tpy) as i32,
                  (belt.0 + tpx) as i32, (belt.1 + tpy) as i32, dark_col);

    // ── Head ──────────────────────────────────────────────────────────────────
    buf.fill_circle(head_cx, head_cy, 5, dark_col);
    buf.fill_circle(head_cx, head_cy, 4, skin_col);
    // Helmet cap — only when no hat equipped (hat replaces it)
    if hat_id == 0 {
        for dy in -5..=0i32 {
            for dx in -4..=4i32 {
                if dx * dx + dy * dy <= 16 {
                    buf.set_pixel(head_cx + dx, head_cy + dy, team_col);
                }
            }
        }
    }
    // Eye
    let eye_x = head_cx + f as i32;
    buf.set_pixel(eye_x,     head_cy + 1, dark_col);
    buf.set_pixel(eye_x + 1, head_cy + 1, dark_col);
    // Hat drawn after head
    if hat_id > 0 { draw_hat(buf, head_cx, head_cy, hat_id); }

    // ── Front leg (after body for correct depth) ──────────────────────────────
    thick_line(buf, hip.0, hip.1, front_knee.0, front_knee.1, dark_col, 7);
    thick_line(buf, hip.0, hip.1, front_knee.0, front_knee.1, body_col, 5);
    thick_line(buf, front_knee.0, front_knee.1, front_foot.0, front_foot.1, dark_col, 7);
    thick_line(buf, front_knee.0, front_knee.1, front_foot.0, front_foot.1, body_col, 5);
    buf.fill_rect(front_foot.0 as i32 - 1, front_foot.1 as i32 - 1, 4, 3, boot_col);

    // ── Front arm ─────────────────────────────────────────────────────────────
    thick_line(buf, arm_orig.0, arm_orig.1, fwd_arm.0, fwd_arm.1, dark_col, 5);
    thick_line(buf, arm_orig.0, arm_orig.1, fwd_arm.0, fwd_arm.1, body_col, 3);

    // ── Gun ───────────────────────────────────────────────────────────────────
    let arm_end = fwd_arm;
    let disp = if f >= 0.0 {
        aim_angle.unwrap_or(0.0)
    } else {
        std::f32::consts::PI - aim_angle.unwrap_or(0.0)
    };
    let (btx, bty) = draw_gun_style(buf, arm_end, disp, gun_style_id);

    // ── HP number ─────────────────────────────────────────────────────────────
    if hp > 0 && show_hp {
        let hat_lift = if hat_id > 0 { 21 } else { 0 };
        draw_hp_number_lifted(buf, pos.x as i32, pos.y as i32, hp, team, hat_lift);
    }

    if aim_angle.is_some() && hp > 0 { Some((btx, bty)) } else { None }
}
