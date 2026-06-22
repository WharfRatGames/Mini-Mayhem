/// Soldier style preview sheet — current vs proposed cartoony redesign.
/// Run: cargo run --bin sprite-sheet
/// Output: assets/soldier_thick_preview.png
use arty::renderer::buffer::WorldBuffer;
use arty::renderer::fb::Bgra;
use arty::renderer::skeleton::{draw_soldier_skeletal, SoldierAnim};
use arty::world::WorldPos;

// ── Shared thick_line (self-contained, no live-code dependency) ───────────────

fn thick_line(buf: &mut WorldBuffer, ax: f32, ay: f32, bx: f32, by: f32, col: Bgra, w: i32) {
    let dx = bx - ax;
    let dy = by - ay;
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let px = ((-dy / len).round()) as i32;
    let py = ((dx / len).round()) as i32;
    let half = w / 2;
    for o in -half..=half {
        buf.draw_line(ax as i32 + px * o, ay as i32 + py * o,
                      bx as i32 + px * o, by as i32 + py * o, col);
    }
}

// ── Bone math (duplicated from skeleton.rs so we don't touch live code) ───────

const TORSO: usize = 0;
const HEAD:  usize = 1;
const ARM_R: usize = 2;
const ARM_L: usize = 3;
const LEG_R: usize = 4;
const LEG_L: usize = 5;
const N_BONES: usize = 6;

struct Bone { parent: Option<usize>, length: f32, angle: f32 }

fn rot(x: f32, y: f32, a: f32) -> (f32, f32) {
    let (s, c) = a.sin_cos();
    (x * c - y * s, x * s + y * c)
}
fn smoothstep(t: f32) -> f32 { t * t * (3.0 - 2.0 * t) }

fn compute_positions(root: (f32, f32), bones: &[Bone; N_BONES]) -> [(f32, f32); N_BONES] {
    let mut world_angles = [0f32; N_BONES];
    let mut ends = [(0f32, 0f32); N_BONES];
    for i in 0..N_BONES {
        let parent_origin = bones[i].parent.map_or(root, |p| ends[p]);
        let parent_angle  = bones[i].parent.map_or(0.0,  |p| world_angles[p]);
        let world_angle = parent_angle + bones[i].angle;
        world_angles[i] = world_angle;
        let (dx, dy) = rot(0.0, -bones[i].length, world_angle);
        ends[i] = (parent_origin.0 + dx, parent_origin.1 + dy);
    }
    ends
}

fn default_bones() -> [Bone; N_BONES] {
    use std::f32::consts::PI;
    [
        Bone { parent: None,        length: 13.0, angle: 0.0 },
        Bone { parent: Some(TORSO), length: 6.0,  angle: 0.0 },
        Bone { parent: Some(TORSO), length: 9.0,  angle: 0.0 },
        Bone { parent: Some(TORSO), length: 9.0,  angle: 0.0 },
        Bone { parent: None,        length: 11.0, angle: PI },
        Bone { parent: None,        length: 11.0, angle: PI },
    ]
}

fn walk_swing_r(tick: u32) -> f32 {
    const STRIDE: f32 = 20.0;
    let phase = (tick as f32 % STRIDE) / STRIDE;
    let t4 = phase * 4.0;
    let frac = smoothstep(t4.fract());
    match t4.floor() as u32 % 4 {
        0 => 1.0 - frac, 1 => -frac, 2 => -1.0 + frac, _ => frac,
    }
}

fn apply_anim(bones: &mut [Bone; N_BONES], anim: &SoldierAnim, f: f32) {
    use std::f32::consts::PI;
    match anim {
        SoldierAnim::Idle => {
            bones[ARM_R].angle =  1.3;
            bones[ARM_L].angle = -1.3;
            bones[LEG_R].angle = PI - 0.05;
            bones[LEG_L].angle = PI + 0.05;
        }
        SoldierAnim::Walking { tick } => {
            let sr = walk_swing_r(*tick);
            bones[TORSO].angle = sr * 0.06;
            bones[LEG_R].angle = PI + sr * 0.6;
            bones[LEG_L].angle = PI - sr * 0.6;
            bones[ARM_R].angle = -sr * 0.26 * f + 0.4;
            bones[ARM_L].angle =  sr * 0.26 * f - 0.4;
        }
        SoldierAnim::Airborne { vel_x, vel_y, airtime, spinning } => {
            if *spinning {
                let angle = -(f) * *airtime as f32 / 18.0 * std::f32::consts::TAU;
                bones[TORSO].angle = angle;
                bones[ARM_R].angle =  0.35;
                bones[ARM_L].angle = -0.35;
                bones[LEG_R].angle = PI + angle + 0.12;
                bones[LEG_L].angle = PI + angle - 0.12;
            } else {
                let lean = (vel_x * 0.025).clamp(-0.5, 0.5);
                let tuck = (-vel_y * 0.03).clamp(-0.4, 0.25);
                bones[TORSO].angle = lean;
                bones[LEG_R].angle = PI + tuck + 0.15;
                bones[LEG_L].angle = PI + tuck - 0.15;
                bones[ARM_R].angle = lean * 0.5 + 0.4;
                bones[ARM_L].angle = lean * 0.5 - 0.4;
            }
        }
        SoldierAnim::Dead => {
            let flop = std::f32::consts::FRAC_PI_2 * f;
            bones[TORSO].angle = flop;
            bones[HEAD].angle  = std::f32::consts::FRAC_PI_4 * f;
            bones[LEG_R].angle = PI + 0.9;
            bones[LEG_L].angle = PI + 0.3;
            bones[ARM_R].angle = -1.1;
            bones[ARM_L].angle =  0.4;
        }
    }
}

fn bone_positions(pos: WorldPos, anim: &SoldierAnim, f: f32) -> (
    (f32,f32), // hip
    [(f32,f32); N_BONES], // ends
    f32, // walk swing r
) {
    let walk_sr = match anim { SoldierAnim::Walking { tick } => walk_swing_r(*tick), _ => 0.0 };
    let bob = walk_sr * walk_sr;
    let rise = (1.0 - bob) * 2.0;
    let root = match anim {
        SoldierAnim::Walking { .. } => (pos.x + walk_sr * 3.0 * f, pos.y - 11.0 - rise),
        _ => (pos.x, pos.y - 11.0),
    };
    let mut bones = default_bones();
    apply_anim(&mut bones, anim, f);
    // Aim arm toward default angle (45°)
    let aim = 0.6f32;
    let aim_disp = if f >= 0.0 { aim } else { std::f32::consts::PI - aim };
    let arm_world = std::f32::consts::FRAC_PI_2 - aim_disp;
    let torso_world = bones[TORSO].angle;
    let arm_local = arm_world - torso_world;
    if f >= 0.0 { bones[ARM_R].angle = arm_local; } else { bones[ARM_L].angle = arm_local; }
    let ends = compute_positions(root, &bones);
    (root, ends, walk_sr)
}

// ── Proposed cartoony soldier ─────────────────────────────────────────────────

fn draw_proposed(buf: &mut WorldBuffer, pos: WorldPos, team: usize, facing: i8, hp: u8, anim: &SoldierAnim) {
    use arty::renderer::draw_sprites::{TEAM_COLOURS, TEAM_COLOURS_DEAD};
    let team_col = if hp == 0 { TEAM_COLOURS_DEAD[team.min(3)] } else { TEAM_COLOURS[team.min(3)] };
    let skin     = Bgra::new(230, 190, 150);
    let dark     = Bgra::new(15,  10,   5);
    let gun_col  = Bgra::new(80,  80,   85);
    let hilit    = Bgra::new(team_col.r.saturating_add(40), team_col.g.saturating_add(30), team_col.b.saturating_add(20));
    let shadow   = Bgra::new(team_col.r.saturating_sub(40), team_col.g.saturating_sub(35), team_col.b.saturating_sub(25));
    let belt_col = Bgra::new(60, 50, 35);
    let buckle   = Bgra::new(200, 170, 60);
    let boot_col = Bgra::new(50,  38,  25);

    let f = facing as f32;

    let (hip, ends, walk_sr) = bone_positions(pos, anim, f);
    let shoulder  = ends[TORSO];
    let arm_r_end = ends[ARM_R];
    let arm_l_end = ends[ARM_L];
    let leg_r_end = ends[LEG_R];
    let leg_l_end = ends[LEG_L];

    let arm_orig = (
        hip.0 + (shoulder.0 - hip.0) * 0.70,
        hip.1 + (shoulder.1 - hip.1) * 0.70,
    );
    let shift = (arm_orig.0 - shoulder.0, arm_orig.1 - shoulder.1);
    let arm_r_vis = (arm_r_end.0 + shift.0, arm_r_end.1 + shift.1);
    let arm_l_vis = (arm_l_end.0 + shift.0, arm_l_end.1 + shift.1);
    let fwd_arm = if f >= 0.0 { arm_r_vis } else { arm_l_vis };

    let bend_r = (1.0 - walk_sr) * 0.5 * 3.5;
    let bend_l = (1.0 + walk_sr) * 0.5 * 3.5;
    let knee_r  = { let m = ((hip.0+leg_r_end.0)*0.5, (hip.1+leg_r_end.1)*0.5); (m.0+f*bend_r, m.1) };
    let knee_l  = { let m = ((hip.0+leg_l_end.0)*0.5, (hip.1+leg_l_end.1)*0.5); (m.0+f*bend_l, m.1) };
    let (back_knee, back_foot, front_knee, front_foot) = if walk_sr >= 0.0 {
        (knee_l, leg_l_end, knee_r, leg_r_end)
    } else {
        (knee_r, leg_r_end, knee_l, leg_l_end)
    };

    // ── Back leg ──────────────────────────────────────────────────────────────
    thick_line(buf, hip.0, hip.1, back_knee.0, back_knee.1, dark, 7);
    thick_line(buf, hip.0, hip.1, back_knee.0, back_knee.1, shadow, 5);
    thick_line(buf, back_knee.0, back_knee.1, back_foot.0, back_foot.1, dark, 7);
    thick_line(buf, back_knee.0, back_knee.1, back_foot.0, back_foot.1, shadow, 5);
    // knee joint dot
    buf.fill_circle(back_knee.0 as i32, back_knee.1 as i32, 3, dark);
    buf.fill_circle(back_knee.0 as i32, back_knee.1 as i32, 2, shadow);
    // boot
    let bfx = back_foot.0 as i32; let bfy = back_foot.1 as i32;
    buf.fill_rect(bfx - 4, bfy - 3, 8, 5, dark);
    buf.fill_rect(bfx - 3, bfy - 2, 7, 4, boot_col);

    // ── Torso ─────────────────────────────────────────────────────────────────
    thick_line(buf, hip.0, hip.1, shoulder.0, shoulder.1, dark, 11);
    thick_line(buf, hip.0, hip.1, shoulder.0, shoulder.1, team_col, 9);
    // chest highlight stripe (upper 40% of torso)
    let chest = (hip.0 + (shoulder.0 - hip.0) * 0.55, hip.1 + (shoulder.1 - hip.1) * 0.55);
    thick_line(buf, chest.0, chest.1, shoulder.0, shoulder.1, hilit, 5);
    // belt
    let belt_y = (hip.1 + (shoulder.1 - hip.1) * 0.25) as i32;
    let belt_x = hip.0 as i32;
    buf.fill_rect(belt_x - 5, belt_y - 1, 10, 3, dark);
    buf.fill_rect(belt_x - 4, belt_y,      8, 1, belt_col);
    buf.fill_rect(belt_x - 1, belt_y - 1,  2, 3, buckle); // buckle

    // ── Front leg ─────────────────────────────────────────────────────────────
    thick_line(buf, hip.0, hip.1, front_knee.0, front_knee.1, dark, 7);
    thick_line(buf, hip.0, hip.1, front_knee.0, front_knee.1, team_col, 5);
    thick_line(buf, front_knee.0, front_knee.1, front_foot.0, front_foot.1, dark, 7);
    thick_line(buf, front_knee.0, front_knee.1, front_foot.0, front_foot.1, team_col, 5);
    // knee joint dot
    buf.fill_circle(front_knee.0 as i32, front_knee.1 as i32, 3, dark);
    buf.fill_circle(front_knee.0 as i32, front_knee.1 as i32, 2, team_col);
    // boot
    let ffx = front_foot.0 as i32; let ffy = front_foot.1 as i32;
    buf.fill_rect(ffx - 4, ffy - 3, 9, 5, dark);
    buf.fill_rect(ffx - 3, ffy - 2, 8, 4, boot_col);
    // boot highlight
    buf.fill_rect(ffx - 3, ffy - 2, 3, 1, Bgra::new(90, 70, 45));

    // ── Shoulder ball joint ───────────────────────────────────────────────────
    buf.fill_circle(arm_orig.0 as i32, arm_orig.1 as i32, 4, dark);
    buf.fill_circle(arm_orig.0 as i32, arm_orig.1 as i32, 3, team_col);

    // ── Arm ───────────────────────────────────────────────────────────────────
    thick_line(buf, arm_orig.0, arm_orig.1, fwd_arm.0, fwd_arm.1, dark, 7);
    thick_line(buf, arm_orig.0, arm_orig.1, fwd_arm.0, fwd_arm.1, team_col, 5);

    // ── Gun ───────────────────────────────────────────────────────────────────
    let disp = if f >= 0.0 { 0.6f32 } else { std::f32::consts::PI - 0.6 };
    let fwd = (disp.cos(), -disp.sin());
    let tip = (fwd_arm.0 + fwd.0 * 14.0, fwd_arm.1 + fwd.1 * 14.0);
    // outline then barrel
    thick_line(buf, fwd_arm.0, fwd_arm.1, tip.0, tip.1, dark, 5);
    thick_line(buf, fwd_arm.0, fwd_arm.1, tip.0, tip.1, gun_col, 3);
    // muzzle cap
    buf.fill_circle(tip.0 as i32, tip.1 as i32, 3, dark);
    buf.fill_circle(tip.0 as i32, tip.1 as i32, 2, gun_col);

    // ── Head ──────────────────────────────────────────────────────────────────
    let hcx = shoulder.0 as i32;
    let hcy = (shoulder.1 - 5.0) as i32;
    // outline
    buf.fill_circle(hcx, hcy, 8, dark);
    // face
    buf.fill_circle(hcx, hcy, 7, skin);
    // helmet (top half, team color)
    for dy in -8i32..=0 {
        for dx in -7i32..=7 {
            if dx * dx + dy * dy <= 49 {
                buf.set_pixel(hcx + dx, hcy + dy, team_col);
            }
        }
    }
    // helmet brim line
    buf.fill_rect(hcx - 7, hcy, 14, 2, dark);
    buf.fill_rect(hcx - 8, hcy + 1, 16, 1, dark);
    // helmet highlight
    buf.fill_rect(hcx - 3, hcy - 6, 3, 2, hilit);

    let fi = f as i32; // +1 right, -1 left
    let eye_x = hcx + fi * 2;

    // ear nub (back of head, opposite facing)
    let ear_x = hcx - fi * 6;
    buf.fill_rect(ear_x - 1, hcy + 1, 3, 4, skin);
    buf.fill_rect(ear_x,     hcy + 2, 1, 2, Bgra::new(200, 155, 115)); // inner ear shadow

    // eyebrow (dark bar above eye, just under brim)
    buf.fill_rect(eye_x - 1, hcy + 1, 4, 1, dark);

    // eye white + pupil
    buf.fill_circle(eye_x, hcy + 3, 2, Bgra::new(220, 220, 220));
    buf.set_pixel(eye_x + fi, hcy + 3, dark); // pupil looking forward
    buf.set_pixel(eye_x,      hcy + 2, Bgra::new(255, 255, 255)); // glint

    // nose dot
    buf.set_pixel(eye_x + fi * 2, hcy + 5, Bgra::new(180, 135, 100));

    // chin shadow (darkened skin strip at jaw)
    let chin_col = Bgra::new(190, 148, 108);
    buf.fill_rect(hcx - 4, hcy + 6, 8, 2, chin_col);

    // mouth — thin line with slight downturn
    buf.fill_rect(eye_x - 1, hcy + 5, 3, 1, dark);
    buf.set_pixel(eye_x - 2, hcy + 6, dark); // left corner down
    buf.set_pixel(eye_x + 2, hcy + 6, dark); // right corner down
}

/// Same as draw_proposed but passes a hat_id — uses draw_soldier_skeletal for
/// the hat sprite (image-based) while drawing the proposed body underneath.
fn draw_proposed_hatted(buf: &mut WorldBuffer, pos: WorldPos, team: usize, facing: i8, hp: u8, anim: &SoldierAnim, hat_id: u8) {
    draw_proposed(buf, pos, team, facing, hp, anim);
    // Overdraw with the live skeletal renderer just for the hat — we call it
    // with hp=0 (no HP badge) and aim_angle=None so it draws a minimal soldier
    // then we rely on hat being drawn on top. To isolate the hat we clear the
    // buffer region first with the bg color, draw the full skeletal (V2 active),
    // then re-draw our proposed body over it — net effect: proposed body + live hat.
    // Simpler: just call the live renderer which now IS the proposed renderer.
    draw_soldier_skeletal(buf, pos, team, facing, hp, anim, Some(0.6), false,
        hat_id, 0, 0, 0, None, 0.0, 0, 0);
}

// ── Canvas ────────────────────────────────────────────────────────────────────

struct Canvas { data: Vec<u8>, w: u32, h: u32 }

impl Canvas {
    fn new(w: u32, h: u32, bg: (u8,u8,u8)) -> Self {
        let mut data = vec![0u8; (w * h * 4) as usize];
        for i in 0..w*h {
            let o = (i*4) as usize;
            data[o] = bg.2; data[o+1] = bg.1; data[o+2] = bg.0; data[o+3] = 255;
        }
        Self { data, w, h }
    }
    fn set(&mut self, x: i32, y: i32, r: u8, g: u8, b: u8) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 { return; }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        self.data[i] = b; self.data[i+1] = g; self.data[i+2] = r; self.data[i+3] = 255;
    }
    fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, r: u8, g: u8, b: u8) {
        for dy in 0..h as i32 { for dx in 0..w as i32 { self.set(x+dx, y+dy, r, g, b); } }
    }
    fn line(&mut self, x: i32, y: i32, w: u32, r: u8, g: u8, b: u8) {
        self.fill_rect(x, y, w, 1, r, g, b);
    }
    fn blit(&mut self, wbuf: &WorldBuffer, sx: i32, sy: i32, sw: u32, sh: u32, dx: i32, dy: i32) {
        for row in 0..sh as i32 {
            for col in 0..sw as i32 {
                let px = wbuf.get_pixel(sx+col, sy+row);
                self.set(dx+col, dy+row, px.r, px.g, px.b);
            }
        }
    }
}

// ── Layout constants ──────────────────────────────────────────────────────────

const CELL_W:  u32 = 90;
const CELL_H:  u32 = 90;
const POSE_COLS: u32 = 5;
const ROWS:    u32 = 4;
const GAP:     u32 = 16;
const HEADER:  u32 = 48;
const LABEL_W: u32 = 50;

// Hat parade: 10 hats per row, 2 rows (current on top, proposed below)
const HAT_COLS: u32 = 10;
const HAT_IDS: &[u8] = &[0,1,2,3,4,5,6,7,8,9]; // first 10 hats; 0=no hat
const HAT_NAMES: &[&str] = &["NONE","HAT1","PROP","HAT3","HAT4","FEZ","HAT6","HAT7","HAT8","HAT9"];

const POSE_W: u32 = LABEL_W + CELL_W * POSE_COLS * 2 + GAP;
const HAT_W:  u32 = LABEL_W + CELL_W * HAT_COLS;
const IMG_W:  u32 = if POSE_W > HAT_W { POSE_W } else { HAT_W };
const POSE_H: u32 = HEADER + CELL_H * ROWS;
const HAT_H:  u32 = 24 + CELL_H * 2 + 16; // header + current row + proposed row + gap
const IMG_H:  u32 = POSE_H + 20 + HAT_H;

fn main() {
    let poses: &[(&str, SoldierAnim)] = &[
        ("IDLE",  SoldierAnim::Idle),
        ("WLK A", SoldierAnim::Walking { tick: 0 }),
        ("WLK B", SoldierAnim::Walking { tick: 10 }),
        ("AIR",   SoldierAnim::Airborne { vel_x: 3.0, vel_y: -4.0, airtime: 0, spinning: false }),
        ("DEAD",  SoldierAnim::Dead),
    ];
    let teams = ["RED", "BLUE", "GRN", "YLW"];

    let mut canvas = Canvas::new(IMG_W, IMG_H, (22, 24, 38));
    let mut wbuf = WorldBuffer::new();

    // ── Pose grid ─────────────────────────────────────────────────────────────
    let cur_cx = (LABEL_W + CELL_W * POSE_COLS / 2) as i32;
    let new_cx = (LABEL_W + CELL_W * POSE_COLS + GAP + CELL_W * POSE_COLS / 2) as i32;
    draw_label(&mut canvas, cur_cx - 28, 8, "CURRENT");
    draw_label(&mut canvas, new_cx - 28, 8, "PROPOSED");

    let div_x = (LABEL_W + CELL_W * POSE_COLS + GAP / 2) as i32;
    canvas.fill_rect(div_x, 0, 2, POSE_H, 70, 72, 100);

    for (ci, (label, _)) in poses.iter().enumerate() {
        let lx_cur = (LABEL_W + ci as u32 * CELL_W) as i32 + 2;
        let lx_new = (LABEL_W + CELL_W * POSE_COLS + GAP + ci as u32 * CELL_W) as i32 + 2;
        draw_label(&mut canvas, lx_cur, 22, label);
        draw_label(&mut canvas, lx_new, 22, label);
    }

    for (ri, team_name) in teams.iter().enumerate() {
        let cell_top = (HEADER + ri as u32 * CELL_H) as i32;
        let foot_y   = cell_top + CELL_H as i32 - 14;
        draw_label(&mut canvas, 2, cell_top + CELL_H as i32 / 2 - 4, team_name);
        if ri > 0 { canvas.line(0, cell_top, IMG_W, 35, 37, 55); }

        for (ci, (_, anim)) in poses.iter().enumerate() {
            let cx_cur = (LABEL_W + ci as u32 * CELL_W) as i32;
            let foot_x_cur = cx_cur + CELL_W as i32 / 2;
            wbuf.fill_rect(cx_cur, cell_top, CELL_W, CELL_H, Bgra::new(22, 24, 38));
            draw_soldier_skeletal(&mut wbuf, WorldPos::new(foot_x_cur as f32, foot_y as f32),
                ri, 1, 100, anim, Some(0.6), false, 0, 0, 0, 0, None, 0.0, 0, 0);
            canvas.blit(&wbuf, cx_cur, cell_top, CELL_W, CELL_H, cx_cur, cell_top);

            let cx_new = (LABEL_W + CELL_W * POSE_COLS + GAP + ci as u32 * CELL_W) as i32;
            let foot_x_new = cx_new + CELL_W as i32 / 2;
            wbuf.fill_rect(cx_new, cell_top, CELL_W, CELL_H, Bgra::new(22, 24, 38));
            draw_proposed(&mut wbuf, WorldPos::new(foot_x_new as f32, foot_y as f32),
                ri, 1, 100, anim);
            canvas.blit(&wbuf, cx_new, cell_top, CELL_W, CELL_H, cx_new, cell_top);
        }
    }

    // ── Hat parade ────────────────────────────────────────────────────────────
    let hat_y0 = (POSE_H + 20) as i32;
    canvas.fill_rect(0, hat_y0 - 2, IMG_W, 2, 50, 52, 75);
    draw_label(&mut canvas, 2, hat_y0 + 4, "HATS");
    draw_label(&mut canvas, (LABEL_W + CELL_W * HAT_COLS / 2) as i32 - 28, hat_y0 + 4, "CURRENT");
    let cur_hat_y = hat_y0 + 24;
    let new_hat_y = cur_hat_y + CELL_H as i32;
    draw_label(&mut canvas, 2, new_hat_y + CELL_H as i32 / 2 - 4, "PROP");

    // Render all hat cells into a fixed wbuf region (top-left corner, always in bounds)
    const BUF_X: i32 = 0;
    const BUF_Y: i32 = 0;
    for (ci, (&hat_id, hat_name)) in HAT_IDS.iter().zip(HAT_NAMES.iter()).enumerate() {
        let dst_cx = (LABEL_W + ci as u32 * CELL_W) as i32;
        let buf_foot_x = BUF_X + CELL_W as i32 / 2;

        draw_label(&mut canvas, dst_cx + 2, hat_y0 + 12, hat_name);

        // current — render at (BUF_X, BUF_Y) then blit to canvas
        let cur_foot_y = BUF_Y + CELL_H as i32 - 14;
        wbuf.fill_rect(BUF_X, BUF_Y, CELL_W, CELL_H, Bgra::new(22, 24, 38));
        draw_soldier_skeletal(&mut wbuf, WorldPos::new(buf_foot_x as f32, cur_foot_y as f32),
            0, 1, 100, &SoldierAnim::Idle, Some(0.6), false,
            hat_id, 0, 0, 0, None, 0.0, 0, 0);
        canvas.blit(&wbuf, BUF_X, BUF_Y, CELL_W, CELL_H, dst_cx, cur_hat_y);

        // proposed
        let new_foot_y = BUF_Y + CELL_H as i32 - 14;
        wbuf.fill_rect(BUF_X, BUF_Y, CELL_W, CELL_H, Bgra::new(22, 24, 38));
        draw_proposed_hatted(&mut wbuf, WorldPos::new(buf_foot_x as f32, new_foot_y as f32),
            0, 1, 100, &SoldierAnim::Idle, hat_id);
        canvas.blit(&wbuf, BUF_X, BUF_Y, CELL_W, CELL_H, dst_cx, new_hat_y);
    }

    // Save
    let path = "assets/soldier_thick_preview.png";
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(file, IMG_W, IMG_H);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&canvas.data).unwrap();
    println!("Wrote {path}  ({IMG_W}×{IMG_H})");
}

// ── 5×7 bitmap font ───────────────────────────────────────────────────────────

fn draw_label(canvas: &mut Canvas, x: i32, y: i32, text: &str) {
    let mut cx = x;
    for ch in text.chars() {
        for (row, &bits) in char_bits(ch).iter().enumerate() {
            for col in 0u8..5 {
                if bits & (1 << (4 - col)) != 0 {
                    canvas.set(cx + col as i32, y + row as i32, 210, 215, 230);
                }
            }
        }
        cx += 6;
    }
}

fn char_bits(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0b01110,0b10001,0b10001,0b11111,0b10001,0b10001,0b10001],
        'B' => [0b11110,0b10001,0b10001,0b11110,0b10001,0b10001,0b11110],
        'C' => [0b01110,0b10001,0b10000,0b10000,0b10000,0b10001,0b01110],
        'D' => [0b11110,0b10001,0b10001,0b10001,0b10001,0b10001,0b11110],
        'E' => [0b11111,0b10000,0b10000,0b11110,0b10000,0b10000,0b11111],
        'G' => [0b01110,0b10001,0b10000,0b10111,0b10001,0b10001,0b01110],
        'I' => [0b01110,0b00100,0b00100,0b00100,0b00100,0b00100,0b01110],
        'K' => [0b10001,0b10010,0b10100,0b11000,0b10100,0b10010,0b10001],
        'L' => [0b10000,0b10000,0b10000,0b10000,0b10000,0b10000,0b11111],
        'N' => [0b10001,0b11001,0b10101,0b10011,0b10001,0b10001,0b10001],
        'O' => [0b01110,0b10001,0b10001,0b10001,0b10001,0b10001,0b01110],
        'P' => [0b11110,0b10001,0b10001,0b11110,0b10000,0b10000,0b10000],
        'R' => [0b11110,0b10001,0b10001,0b11110,0b10100,0b10010,0b10001],
        'S' => [0b01111,0b10000,0b10000,0b01110,0b00001,0b00001,0b11110],
        'T' => [0b11111,0b00100,0b00100,0b00100,0b00100,0b00100,0b00100],
        'U' => [0b10001,0b10001,0b10001,0b10001,0b10001,0b10001,0b01110],
        'W' => [0b10001,0b10001,0b10001,0b10101,0b10101,0b11011,0b10001],
        'X' => [0b10001,0b01010,0b00100,0b00100,0b00100,0b01010,0b10001],
        'Y' => [0b10001,0b10001,0b01010,0b00100,0b00100,0b00100,0b00100],
        ' ' => [0;7],
        _   => [0b00100,0b00100,0b00100,0b00000,0b00100,0b00000,0b00100],
    }
}
