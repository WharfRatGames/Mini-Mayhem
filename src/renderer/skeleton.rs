use crate::world::WorldPos;
use crate::physics::projectile::WeaponKind;
use super::buffer::WorldBuffer;
use super::fb::Bgra;
use super::draw_sprites::{TEAM_COLOURS, TEAM_COLOURS_DEAD, draw_hp_number_lifted};
use super::cosmetic_sprites::draw_boot;

// Flip to false to revert to the original soldier style.
pub const SOLDIER_STYLE_V2: bool = true;

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
    bones[ARM_R].angle = 1.3;  // arms hang at sides, gun at waist
    bones[ARM_L].angle = -1.3;
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

// ── Hat drawing ───────────────────────────────────────────────────────────────

fn draw_hat(buf: &mut WorldBuffer, cx: i32, cy: i32, hat_id: u8, wind: f32, tick: u32, facing: f32) {
    if hat_id == 0 { return; }
    // Render the real shop-icon sprite, scaled up (32x29, 1.45x the
    // documented 22x20 game-px size) for in-game readability.
    // Per COSMETIC_STYLE_GUIDE.md the sprite's head-anchor pixel is (33,45)
    // of 66x60 -> 5px below sprite centre at this size's ~7px (1.45x);
    // shift the centred blit up so the anchor lands on the head centre.
    let (w, h, anchor_dy, hat_dy) = if SOLDIER_STYLE_V2 {
        let base_w = 46i32;
        let base_h = 42i32;
        let anchor = 13i32;
        let dy: i32 = match hat_id {
            5  => 8,
            15 => 14,
            28 => 12,
            33 => 7,
            36 => 12,
            _  => 0,
        };
        let scale: f32 = match hat_id {
            22 => 0.75,
            28 => 0.80,
            36 => 0.55,
            _  => 1.0,
        };
        ((base_w as f32 * scale) as i32, (base_h as f32 * scale) as i32, anchor, dy)
    } else {
        let base_w = 40i32;
        let base_h = 36i32;
        let anchor = 9i32;
        let dy: i32 = match hat_id {
            5  => 5,
            15 => 11,
            28 => 9,
            33 => 4,
            36 => 9,
            _  => 0,
        };
        let scale: f32 = match hat_id {
            22 => 0.75,
            28 => 0.80,
            36 => 0.48,
            _  => 1.0,
        };
        ((base_w as f32 * scale) as i32, (base_h as f32 * scale) as i32, anchor, dy)
    };
    // Dragon Skull (36) flips to face the direction the soldier faces
    let flip = hat_id == 36 && facing < 0.0;
    super::cosmetic_sprites::draw_hat(buf, hat_id, cx, cy - anchor_dy + hat_dy, w, h, flip);

    // Propeller Hat: the sprite's static propeller bar (source rows 18-26) is
    // skipped by cosmetic_sprites::draw_hat for hat_id 2; draw an animated
    // spinning propeller in its place, matching wind direction/speed.
    if hat_id == 2 {
        let blade = Bgra::new(230, 230, 230); // sampled from hat_2.png propeller bar
        let hub_x = cx as f32;
        let hub_y = (cy - anchor_dy - h / 2 + 13) as f32; // centre of skipped band, scaled to render space
        let dir = if wind >= 0.0 { 1.0 } else { -1.0 };
        let speed = 1.0 + wind.abs() * 5.0;
        let frame = (tick as f32 / 4.0).floor() * dir * speed;
        let angle = frame * std::f32::consts::FRAC_PI_4; // 45° steps
        let half_len = 4.0;
        // Profile view: the blade lies parallel to the hat brim (horizontal)
        // and spins edge-on, so only its apparent length foreshortens with
        // rotation — it never tilts up/down.
        let dx = angle.cos() * half_len;
        thick_line(buf, hub_x - dx, hub_y, hub_x + dx, hub_y, blade, 2);
    }
}

// ── Gun style drawing ─────────────────────────────────────────────────────────

/// Draw the gun and return the barrel tip (x, y) for the charge meter origin.
/// `origin` is the forward arm endpoint. `disp` is the display angle (already
/// adjusted for facing direction). All styles use fwd + perp offset math so
/// they rotate correctly with the aim angle.
fn draw_gun_style(buf: &mut WorldBuffer, origin: (f32, f32), disp: f32, gun_style_id: u8) -> (f32, f32) {
    draw_gun_style_len(buf, origin, disp, gun_style_id, 17.0)
}

fn draw_gun_style_len(buf: &mut WorldBuffer, origin: (f32, f32), disp: f32, gun_style_id: u8, length_px: f32) -> (f32, f32) {
    let fwd = (disp.cos(), -disp.sin());
    let prp = (disp.sin(),  disp.cos());
    super::cosmetic_sprites::draw_gun_oriented(buf, gun_style_id, origin, fwd, prp, length_px)
}

// ── Held weapon draw ─────────────────────────────────────────────────────────

fn draw_held_weapon(buf: &mut WorldBuffer, x: i32, y: i32, weapon: WeaponKind, tick: u32) {
    match weapon {
        WeaponKind::Grenade => {
            super::draw_sprites::draw_grenade_projectile(buf, WorldPos::new(x as f32, y as f32));
        }
        WeaponKind::HolyHandGrenade => {
            // Golden orb body (frame 0 — upright)
            let gdark = Bgra::new(140, 95, 10);
            let gbody = Bgra::new(210, 155, 30);
            let ghi   = Bgra::new(255, 230, 100);
            let gray  = Bgra::new(160, 160, 165);
            let gold  = Bgra::new(255, 215, 45);
            buf.fill_rect(x - 3, y - 5, 6, 12, gdark);
            buf.fill_rect(x - 2, y - 4, 4,  9, gbody);
            buf.fill_rect(x - 1, y - 3, 2,  1, ghi);
            buf.fill_rect(x - 3, y - 1, 6,  1, gdark); // equator band
            // Cross on top
            buf.fill_rect(x - 1, y - 7, 3, 3, gdark);
            buf.fill_rect(x,     y - 7, 1, 3, gold);
            buf.fill_rect(x - 2, y - 6, 5, 1, gold);
            // Pin
            buf.fill_rect(x - 1, y + 5, 2, 2, gray);
            let _ = ghi;
        }
        WeaponKind::BananaBomb => {
            // Gray sphere (main meteor bomb body)
            buf.fill_circle(x, y, 7, Bgra::new(55, 55, 58));
            buf.fill_circle(x, y, 6, Bgra::new(130, 130, 135));
            buf.fill_circle(x - 2, y - 2, 2, Bgra::new(190, 190, 195));
        }
        WeaponKind::BlackHoleBomb => {
            let a = tick as f32 * 0.35;
            let purpd = Bgra::new(60, 0, 90);
            let purp  = Bgra::new(160, 0, 220);
            let void  = Bgra::new(0, 0, 0);
            let glow  = Bgra::new(200, 80, 255);
            buf.fill_circle(x, y, 7, purpd);
            buf.fill_circle(x, y, 5, purp);
            buf.fill_circle(x, y, 3, void);
            for &off in &[0.0f32, std::f32::consts::PI] {
                let gx = x + (6.0 * (a + off).cos()) as i32;
                let gy = y + (6.0 * (a + off).sin()) as i32;
                buf.set_pixel(gx, gy, glow);
            }
        }
        WeaponKind::Blasthive => {
            // Hive: amber rounded box
            let hdk = Bgra::new(70, 45, 12);
            let hmd = Bgra::new(165, 110, 35);
            buf.fill_circle(x, y, 5, hdk);
            buf.fill_circle(x, y, 4, hmd);
            buf.draw_line(x - 3, y - 1, x + 3, y - 1, hdk);
            buf.draw_line(x - 3, y + 1, x + 3, y + 1, hdk);
        }
        WeaponKind::Tnt => {
            // Red stick with gray fuse
            buf.fill_rect(x - 3, y - 7, 6, 12, Bgra::new(190, 25, 15));
            buf.fill_rect(x - 3, y - 7, 2, 12, Bgra::new(230, 60, 45));
            buf.fill_rect(x + 1, y - 7, 2, 12, Bgra::new(110, 12,  8));
            buf.fill_rect(x,     y - 9, 1,  3, Bgra::new(160, 160, 160));
            buf.fill_rect(x + 1, y - 11, 1, 2, Bgra::new(160, 160, 160));
        }
        WeaponKind::Landmine => {
            // Flat disc shape
            let dk  = Bgra::new(15,  50, 15);
            let mid = Bgra::new(40, 100, 30);
            let hi  = Bgra::new(60, 135, 45);
            let rim = Bgra::new(30,  75, 22);
            buf.fill_rect(x - 5, y + 2, 11, 1, dk);
            buf.fill_rect(x - 7, y,     15, 2, mid);
            buf.fill_rect(x - 7, y - 2, 15, 2, mid);
            buf.fill_rect(x - 6, y - 3, 13, 1, rim);
            buf.fill_rect(x - 5, y - 4, 11, 1, dk);
            buf.set_pixel(x - 7, y + 1, rim);
            buf.set_pixel(x + 7, y + 1, rim);
            buf.fill_rect(x - 4, y - 3, 9, 1, hi);
            if (tick / 15) % 2 == 0 {
                buf.fill_rect(x - 1, y - 5, 3, 2, Bgra::new(220, 25, 25));
                buf.set_pixel(x, y - 5, Bgra::new(255, 110, 110));
            } else {
                buf.fill_rect(x - 1, y - 5, 3, 2, Bgra::new(80, 10, 10));
            }
        }
        _ => {}
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
    held_weapon:      Option<WeaponKind>,
    wind:             f32,
    tick:             u32,
    on_fire_ticks:    u32,
) -> Option<(f32, f32)> {
    let team_col = if hp == 0 { TEAM_COLOURS_DEAD[team.min(3)] } else { TEAM_COLOURS[team.min(3)] };
    // body_col: uniform override for torso/arms/legs; helmet cap always keeps team_col
    let body_col = if uniform_color_id == 0 { team_col } else { uniform_color(uniform_color_id) };
    let skin_col = Bgra::new(218, 178, 140);
    let dark_col = Bgra::new(22,  14,  6);

    let f = facing as f32; // +1 right, -1 left

    // Hip = root; Walking gets lateral sway + body rise at passing phase
    let mut root = match anim {
        SoldierAnim::Walking { tick } => {
            let sr = walk_swing_r(*tick);
            let bob = sr * sr; // 1 at contact, 0 at passing
            let rise = (1.0 - bob) * 2.0; // body rises 2px at passing, sits at contact
            (pos.x + sr * 3.0 * f, pos.y - 11.0 - rise)
        }
        _ => (pos.x, pos.y - 11.0),
    };

    // On fire: squirm with small fast jitter.
    if on_fire_ticks > 0 {
        let seed = tick.wrapping_add((pos.x as u32).wrapping_mul(17));
        let jx = ((seed % 7) as f32 - 3.0) * 1.2;
        let jy = (((seed / 7) % 5) as f32 - 2.0) * 1.0;
        root.0 += jx;
        root.1 += jy;
    }

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

    let fwd_arm = if f >= 0.0 { arm_r_vis } else { arm_l_vis };

    let (btx, bty) = if SOLDIER_STYLE_V2 {
        // ── V2: cartoony style ────────────────────────────────────────────────
        let hilit    = Bgra::new(body_col.r.saturating_add(40), body_col.g.saturating_add(30), body_col.b.saturating_add(20));
        let shadow   = Bgra::new(body_col.r.saturating_sub(40), body_col.g.saturating_sub(35), body_col.b.saturating_sub(25));
        let belt_col = Bgra::new(60,  50,  35);
        let buckle   = Bgra::new(200, 170, 60);
        let boot_col = Bgra::new(50,  38,  25);
        let boot_hi  = Bgra::new(90,  70,  45);
        let gun_col  = Bgra::new(80,  80,  85);

        // Back leg (shadow color so it reads as behind the body)
        thick_line(buf, hip.0, hip.1, back_knee.0, back_knee.1, dark_col, 7);
        thick_line(buf, hip.0, hip.1, back_knee.0, back_knee.1, shadow, 5);
        thick_line(buf, back_knee.0, back_knee.1, back_foot.0, back_foot.1, dark_col, 7);
        thick_line(buf, back_knee.0, back_knee.1, back_foot.0, back_foot.1, shadow, 5);
        buf.fill_circle(back_knee.0 as i32, back_knee.1 as i32, 3, dark_col);
        buf.fill_circle(back_knee.0 as i32, back_knee.1 as i32, 2, shadow);
        let bfx = back_foot.0 as i32; let bfy = back_foot.1 as i32;
        buf.fill_rect(bfx - 4, bfy - 3, 8, 5, dark_col);
        buf.fill_rect(bfx - 3, bfy - 2, 7, 4, boot_col);

        // Torso (wider, with chest highlight + belt)
        thick_line(buf, hip.0, hip.1, shoulder.0, shoulder.1, dark_col, 11);
        thick_line(buf, hip.0, hip.1, shoulder.0, shoulder.1, body_col, 9);
        let chest = (hip.0 + (shoulder.0 - hip.0) * 0.55, hip.1 + (shoulder.1 - hip.1) * 0.55);
        thick_line(buf, chest.0, chest.1, shoulder.0, shoulder.1, hilit, 5);
        let belt_pt = (hip.0 + (shoulder.0 - hip.0) * 0.25, hip.1 + (shoulder.1 - hip.1) * 0.25);
        let bx = belt_pt.0 as i32; let by = belt_pt.1 as i32;
        buf.fill_rect(bx - 5, by - 1, 10, 3, dark_col);
        buf.fill_rect(bx - 4, by,      8, 1, belt_col);
        buf.fill_rect(bx - 1, by - 1,  2, 3, buckle);

        // Head (bigger, helmet brim, eye circle, mouth)
        if hat_id != 15 && hat_id != 36 {
            buf.fill_circle(head_cx, head_cy, 8, dark_col);
            buf.fill_circle(head_cx, head_cy, 7, skin_col);
        }
        if hat_id == 0 {
            for dy in -8..=0i32 {
                for dx in -7..=7i32 {
                    if dx * dx + dy * dy <= 49 {
                        buf.set_pixel(head_cx + dx, head_cy + dy, team_col);
                    }
                }
            }
        }
        // Helmet brim
        if hat_id == 0 {
            buf.fill_rect(head_cx - 8, head_cy,     16, 2, dark_col);
            buf.fill_rect(head_cx - 3, head_cy - 6,  3, 2, hilit);
        }
        // Face features — skipped for masks/replacements that cover the whole face
        if hat_id != 28 && hat_id != 15 && hat_id != 36 {
            let fi = f as i32;
            let eye_x = head_cx + fi * 2;

            // Ear nub (back of head, opposite facing)
            let ear_x = head_cx - fi * 6;
            buf.fill_rect(ear_x - 1, head_cy + 1, 3, 4, skin_col);
            buf.fill_rect(ear_x,     head_cy + 2, 1, 2, Bgra::new(190, 148, 108));

            // Eyebrow
            buf.fill_rect(eye_x - 1, head_cy + 1, 4, 1, dark_col);

            // Eye white + pupil + glint
            buf.fill_circle(eye_x, head_cy + 3, 2, Bgra::new(220, 220, 220));
            buf.set_pixel(eye_x + fi, head_cy + 3, dark_col);
            buf.set_pixel(eye_x,      head_cy + 2, Bgra::new(255, 255, 255));

            // Nose dot
            buf.set_pixel(eye_x + fi * 2, head_cy + 5, Bgra::new(180, 135, 100));

            // Chin shadow
            buf.fill_rect(head_cx - 4, head_cy + 6, 8, 2, Bgra::new(190, 148, 108));

            // Mouth with downturned corners
            buf.fill_rect(eye_x - 1, head_cy + 5, 3, 1, dark_col);
            buf.set_pixel(eye_x - 2,  head_cy + 6, dark_col);
            buf.set_pixel(eye_x + 2,  head_cy + 6, dark_col);
        }
        if hat_id > 0 { draw_hat(buf, head_cx, head_cy, hat_id, wind, tick, f); }

        // Front leg
        thick_line(buf, hip.0, hip.1, front_knee.0, front_knee.1, dark_col, 7);
        thick_line(buf, hip.0, hip.1, front_knee.0, front_knee.1, body_col, 5);
        thick_line(buf, front_knee.0, front_knee.1, front_foot.0, front_foot.1, dark_col, 7);
        thick_line(buf, front_knee.0, front_knee.1, front_foot.0, front_foot.1, body_col, 5);
        buf.fill_circle(front_knee.0 as i32, front_knee.1 as i32, 3, dark_col);
        buf.fill_circle(front_knee.0 as i32, front_knee.1 as i32, 2, body_col);
        let ffx = front_foot.0 as i32; let ffy = front_foot.1 as i32;
        buf.fill_rect(ffx - 4, ffy - 3, 9, 5, dark_col);
        buf.fill_rect(ffx - 3, ffy - 2, 8, 4, boot_col);
        buf.fill_rect(ffx - 3, ffy - 2, 3, 1, boot_hi);

        // Shoulder ball joint
        buf.fill_circle(arm_orig.0 as i32, arm_orig.1 as i32, 4, dark_col);
        buf.fill_circle(arm_orig.0 as i32, arm_orig.1 as i32, 3, body_col);

        // Arm (wider)
        thick_line(buf, arm_orig.0, arm_orig.1, fwd_arm.0, fwd_arm.1, dark_col, 7);
        thick_line(buf, arm_orig.0, arm_orig.1, fwd_arm.0, fwd_arm.1, body_col, 5);

        // Gun / held item
        let disp = if f >= 0.0 { aim_angle.unwrap_or(0.0) } else { std::f32::consts::PI - aim_angle.unwrap_or(0.0) };
        if let Some(weapon) = held_weapon {
            draw_held_weapon(buf, fwd_arm.0 as i32, fwd_arm.1 as i32, weapon, tick);
            (fwd_arm.0, fwd_arm.1)
        } else {
            // Thicker barrel outline before the cosmetic gun sprite
            let fwd_dir = (disp.cos(), -disp.sin());
            let tip = (fwd_arm.0 + fwd_dir.0 * 22.0, fwd_arm.1 + fwd_dir.1 * 22.0);
            thick_line(buf, fwd_arm.0, fwd_arm.1, tip.0, tip.1, dark_col, 5);
            let (tx, ty) = draw_gun_style_len(buf, fwd_arm, disp, gun_style_id, 22.0);
            // Muzzle cap
            buf.fill_circle(tx as i32, ty as i32, 3, dark_col);
            buf.fill_circle(tx as i32, ty as i32, 2, gun_col);
            (tx, ty)
        }
    } else {
        // ── V1: original style ────────────────────────────────────────────────
        // Back leg
        thick_line(buf, hip.0, hip.1, back_knee.0, back_knee.1, dark_col, 7);
        thick_line(buf, hip.0, hip.1, back_knee.0, back_knee.1, body_col, 5);
        thick_line(buf, back_knee.0, back_knee.1, back_foot.0, back_foot.1, dark_col, 7);
        thick_line(buf, back_knee.0, back_knee.1, back_foot.0, back_foot.1, body_col, 5);
        draw_boot(buf, boot_color_id, back_foot.0 as i32, back_foot.1 as i32, 5, 4, f < 0.0);

        // Torso
        thick_line(buf, hip.0, hip.1, shoulder.0, shoulder.1, dark_col, 7);
        thick_line(buf, hip.0, hip.1, shoulder.0, shoulder.1, body_col, 5);
        let belt = ((hip.0 + shoulder.0) / 2.0, (hip.1 + shoulder.1) / 2.0);
        let tdx = shoulder.0 - hip.0;
        let tdy = shoulder.1 - hip.1;
        let tlen = (tdx * tdx + tdy * tdy).sqrt().max(0.001);
        let tpx = (-tdy / tlen) * 3.0;
        let tpy = ( tdx / tlen) * 3.0;
        buf.draw_line((belt.0 - tpx) as i32, (belt.1 - tpy) as i32,
                      (belt.0 + tpx) as i32, (belt.1 + tpy) as i32, dark_col);

        // Head
        if hat_id != 15 && hat_id != 36 {
            buf.fill_circle(head_cx, head_cy, 5, dark_col);
            buf.fill_circle(head_cx, head_cy, 4, skin_col);
        }
        if hat_id == 0 {
            for dy in -5..=0i32 {
                for dx in -4..=4i32 {
                    if dx * dx + dy * dy <= 16 {
                        buf.set_pixel(head_cx + dx, head_cy + dy, team_col);
                    }
                }
            }
        }
        if hat_id != 28 && hat_id != 15 && hat_id != 36 {
            let eye_x = head_cx + f as i32;
            buf.set_pixel(eye_x,     head_cy + 1, dark_col);
            buf.set_pixel(eye_x + 1, head_cy + 1, dark_col);
        }
        if hat_id > 0 { draw_hat(buf, head_cx, head_cy, hat_id, wind, tick, f); }

        // Front leg
        thick_line(buf, hip.0, hip.1, front_knee.0, front_knee.1, dark_col, 7);
        thick_line(buf, hip.0, hip.1, front_knee.0, front_knee.1, body_col, 5);
        thick_line(buf, front_knee.0, front_knee.1, front_foot.0, front_foot.1, dark_col, 7);
        thick_line(buf, front_knee.0, front_knee.1, front_foot.0, front_foot.1, body_col, 5);
        draw_boot(buf, boot_color_id, front_foot.0 as i32, front_foot.1 as i32, 5, 4, f < 0.0);

        // Arm
        thick_line(buf, arm_orig.0, arm_orig.1, fwd_arm.0, fwd_arm.1, dark_col, 5);
        thick_line(buf, arm_orig.0, arm_orig.1, fwd_arm.0, fwd_arm.1, body_col, 3);

        // Gun / held item
        let disp = if f >= 0.0 { aim_angle.unwrap_or(0.0) } else { std::f32::consts::PI - aim_angle.unwrap_or(0.0) };
        if let Some(weapon) = held_weapon {
            draw_held_weapon(buf, fwd_arm.0 as i32, fwd_arm.1 as i32, weapon, tick);
            (fwd_arm.0, fwd_arm.1)
        } else {
            draw_gun_style(buf, fwd_arm, disp, gun_style_id)
        }
    };

    // ── HP number ─────────────────────────────────────────────────────────────
    if hp > 0 && show_hp {
        let hat_lift = if hat_id > 0 { 21 } else { 0 };
        draw_hp_number_lifted(buf, pos.x as i32, pos.y as i32, hp, team, hat_lift);
    }

    if aim_angle.is_some() && hp > 0 { Some((btx, bty)) } else { None }
}
