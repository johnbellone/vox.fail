//! Walking, looking and building from the input actions.

use super::*;
use vox_core::body::{Occupancy, with_props};

/// Walking, flying, looking, choosing and building.
///
/// Diablo camera (the default): WASD or the left stick walk relative to the screen, a left
/// click (or holding it) walks to the point under the mouse, the wheel zooms, Q and E turn the
/// view, right click builds where the mouse points, Shift + right click takes a block away.
/// Behind camera (V switches): the mouse looks round once captured and the crosshair builds.
#[allow(clippy::too_many_arguments)]
/// Walking, looking and building, from [`crate::input::Actions`] (keyboard and mouse or a
/// gamepad, whichever is in use). Held break and place repeat, as in a creative world.
#[allow(clippy::too_many_arguments)]
pub fn controls(
    mut w: ResMut<WorldClient>,
    mut av: ResMut<crate::LocalAvatar>,
    actions: Res<crate::input::Actions>,
    settings: Res<crate::input::InputSettings>,
    active: Res<crate::input::ActiveDevice>,
    mode: Res<crate::input::UiMode>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<crate::WorldCamera>>,
    real: Res<Time<Real>>,
    mut walkers: Query<&mut Transform, With<Walker>>,
    mut shown: Query<&mut Visibility>,
    mut repeat: Local<BuildRepeat>,
    // the quick wheel (if the HUD is in) and the prop cells that block walking (if props are in);
    // one parameter, to stay within Bevy's sixteen
    overlay: (Option<Res<crate::hud::QuickWheel>>, Option<Res<crate::props::PropOccupancy>>),
    mut editor: Option<ResMut<crate::editor::EditorState>>,
) {
    let (wheel, props) = overlay;
    // this frame's click is not known to be a pick until the mouse is looked at below
    if let Some(e) = editor.as_mut() {
        e.pointer_claimed = false;
    }
    if !w.active() {
        return;
    }
    let cmds = std::mem::take(&mut w.cmds);
    for c in cmds {
        run_command(&mut w, &c);
    }
    if !w.ready || w.watch {
        // arriving (or through a portal): a button already held then doesn't count
        repeat.armed = false;
        return;
    }
    // a menu is open: it has the input, and the mouse
    if *mode == crate::input::UiMode::Menu {
        if let Ok((_, mut c)) = windows.single_mut() {
            if w.captured {
                c.grab_mode = CursorGrabMode::None;
                c.visible = true;
                w.captured = false;
            }
        }
        repeat.held = None;
        repeat.armed = false;
        return;
    }
    use crate::input::Action as A;
    let dt = real.delta_secs().min(0.25);
    let size = w.block_size;
    let now = w.now();
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let pad_active = active.is_gamepad();
    // edit mode is the editor's state being there (EditorPlugin is added only then): the player
    // always flies, at the edit flight's speed (editor::camera)
    let edit = editor.is_some();
    let fly_speed = if edit { crate::editor::camera::EDIT_FLY } else { FLY };
    // with an editor tool that takes the click (Select picks, Portal places a doorway: see
    // Tool::consumes_click) a click neither breaks a block nor walks to the point
    let click_taken = editor.as_ref().and_then(|e| e.tool).is_some_and(|t| t.consumes_click());
    // no props in the world (or none loaded): the blocks alone stop the body
    let no_props = Occupancy::default();
    let occupancy = props.as_ref().map_or(&no_props, |p| &p.0);
    if actions.just_pressed(A::View) {
        w.cam_mode = w.cam_mode.next();
        let s = w.cam_mode.label();
        w.notice(s);
    }
    let diablo = w.cam_mode == CamMode::Diablo;
    // your own body is out of sight in first person
    if let Some(root) = av.root {
        if let Ok(mut v) = shown.get_mut(root) {
            let want = if w.cam_mode == CamMode::First { Visibility::Hidden } else { Visibility::Inherited };
            if *v != want {
                *v = want;
            }
        }
    }
    // ---- the mouse: captured for looking (first person, behind); a pointer from above.
    // The click that captures it does nothing else.
    let clicked = (mouse.just_pressed(MouseButton::Left), mouse.just_pressed(MouseButton::Right));
    let mut swallowed = false;
    let mut cursor_at = None;
    if let Ok((win, mut c)) = windows.single_mut() {
        if diablo {
            if w.captured {
                c.grab_mode = CursorGrabMode::None;
                c.visible = true;
                w.captured = false;
            }
            cursor_at = win.cursor_position();
        } else if !w.captured && (clicked.0 || clicked.1) && !pad_active {
            c.grab_mode = CursorGrabMode::Locked;
            c.visible = false;
            w.captured = true;
            swallowed = true;
        }
    }
    if let Some(e) = editor.as_mut() {
        e.pointer_claimed = swallowed;
    }
    // the ray under the pointer, from where the camera was last frame
    w.pick = None;
    if let (Some(pos), Ok((cam, gt))) = (cursor_at, cams.single()) {
        if let Ok(ray) = cam.viewport_to_world(gt, pos) {
            w.pick = Some((ray.origin / size, Vec3::from(ray.direction)));
        }
    }
    // ---- look and zoom (not while the quick wheel has the mouse and stick)
    let wheel_open = wheel.is_some_and(|q| q.open);
    if w.captured && !diablo && !wheel_open {
        w.cam_yaw -= actions.mouse_delta.x * settings.mouse_look;
        w.cam_pitch -= actions.mouse_delta.y * settings.mouse_look;
    }
    let look = if wheel_open { Vec2::ZERO } else { actions.look_stick };
    if look != Vec2::ZERO {
        w.cam_yaw -= look.x * settings.stick_look * dt;
        if diablo {
            w.zoom = (w.zoom * (1.0 - look.y * 1.5 * dt)).clamp(4.0, 45.0);
        } else {
            w.cam_pitch += look.y * settings.stick_look * 0.75 * dt;
        }
    }
    w.cam_pitch = w.cam_pitch.clamp(-1.45, 1.3);
    if diablo {
        // Q and R turn the view an eighth of a turn
        if keys.just_pressed(KeyCode::KeyQ) {
            w.cam_yaw += std::f32::consts::FRAC_PI_4;
        }
        if keys.just_pressed(KeyCode::KeyR) {
            w.cam_yaw -= std::f32::consts::FRAC_PI_4;
        }
    }
    // ---- moving
    let mut stick = actions.movement;
    let mut jump = actions.just_pressed(A::Jump);
    jump |= std::mem::take(&mut w.auto_jump);
    if let Some((dir, until)) = w.auto {
        if now < until {
            stick += dir;
        } else {
            w.auto = None;
        }
    }
    let up = actions.pressed(A::Jump);
    let down = actions.pressed(A::Down);
    let run = actions.pressed(A::Sprint);
    // fly: its own button, or jump twice quickly
    let mut toggle_fly = actions.just_pressed(A::Fly);
    if actions.just_pressed(A::Jump) {
        if now - repeat.last_jump < 0.3 {
            toggle_fly = true;
            repeat.last_jump = -1.0;
        } else {
            repeat.last_jump = now;
        }
    }
    // ---- the hotbar
    if let Some(i) = actions.digit {
        if i < w.hotbar.len() {
            w.slot = i;
        }
    }
    let mut cycle = actions.just_pressed(A::HotbarNext) as i32 - actions.just_pressed(A::HotbarPrev) as i32;
    if actions.scroll.abs() > 0.0 {
        if diablo && !shift {
            // the wheel zooms the camera in and out
            w.zoom = (w.zoom * (1.0 - actions.scroll.signum() * 0.12)).clamp(4.0, 45.0);
        } else {
            cycle -= actions.scroll.signum() as i32;
        }
    }
    if cycle != 0 && !w.hotbar.is_empty() {
        let n = w.hotbar.len() as i32;
        w.slot = ((w.slot as i32 + cycle).rem_euclid(n)) as usize;
    }
    if edit {
        // nothing turns the edit flight off
        if !w.fly {
            w.fly = true;
            w.vy = 0.0;
        }
    } else if toggle_fly {
        w.fly = !w.fly;
        w.vy = 0.0;
        let s = if w.fly { "Flying" } else { "Walking" };
        w.notice(s);
    }
    // ---- breaking and placing: on press, then again while held as long as the aim stays put
    // (holding the button and looking round must not sweep blocks away)
    let turned = actions.mouse_delta.length() * settings.mouse_look * w.captured as i32 as f32
        + actions.look_stick.length() * settings.stick_look * dt;
    let (mut place, mut remove) = (false, false);
    if diablo && !pad_active {
        // from above, the pointer builds: right click places, shift and right click removes
        place = clicked.1 && !shift;
        remove = clicked.1 && shift;
    } else if !swallowed {
        let mut held = (actions.pressed(A::Break), actions.pressed(A::Place));
        let mut just = (actions.just_pressed(A::Break), actions.just_pressed(A::Place));
        // the click of a tool that takes it is the Break action; it must not reach the repeat
        // (zeroed before the step, so the repeat timer does not latch on it)
        if click_taken {
            held.0 = false;
            just.0 = false;
        }
        (remove, place) = repeat.step(held, just, now, turned);
    }
    let pick = actions.just_pressed(A::Pick);
    // ---- where to walk
    let stick = stick.clamp_length_max(1.0);
    // "forward" is up the screen: the camera's direction on the ground; through a cut to a
    // fixed camera it stays as it was until you let go
    let screen_yaw = {
        let d = w.cam_dir;
        if d.x.abs() + d.z.abs() > 1e-3 { (-d.x).atan2(-d.z) } else { w.cam_yaw }
    };
    let basis = if stick.length() > 0.1 {
        *w.move_yaw.get_or_insert(screen_yaw)
    } else {
        w.move_yaw = None;
        screen_yaw
    };
    let fwd = Vec3::new(-basis.sin(), 0.0, -basis.cos());
    let right = Vec3::new(basis.cos(), 0.0, -basis.sin());
    let mut dir = fwd * stick.y + right * stick.x;
    // a click (or holding the button) on the ground walks there
    if diablo
        && mouse.pressed(MouseButton::Left)
        && !shift
        && !click_taken
        && let Some((o, d)) = w.pick
        && let Some(h) = ray::raycast_with(|c| w.solid_at(c), o.to_array(), d.to_array(), 160.0)
    {
        let p = IVec3::from(h.cell).as_vec3() + Vec3::new(0.5, 1.0, 0.5);
        w.goal = Some(p);
    }
    if dir.length() > 0.1 {
        w.goal = None;
    } else if let Some(g) = w.goal {
        let to = Vec3::new(g.x - w.body.pos[0], 0.0, g.z - w.body.pos[2]);
        if to.length() < 0.4 {
            w.goal = None;
        } else {
            dir = to.normalize() * (to.length() / 2.0).clamp(0.35, 1.0);
        }
    }
    // a fly-to (the page's go-to, editor::camera) gives way to the player's own motion: any
    // walking, flying or click-to-walk cancels it; while it runs, the keys add nothing, so the
    // total speed stays at the flight's
    let mut flying_to = false;
    if let Some(s) = editor.as_mut().filter(|s| s.fly_to.is_some()) {
        if dir.length() > 0.1 || up || down {
            s.fly_to = None;
        } else {
            flying_to = true;
        }
    }
    if flying_to {
        dir = Vec3::ZERO;
    }
    let speed = if w.fly { fly_speed } else if run { RUN } else { WALK } / size;
    // in steps of at most 50 ms, so a slow frame (or a slow machine) still moves in real time
    let frame = real.delta_secs().min(0.25);
    let steps = (frame / 0.05).ceil().max(1.0) as usize;
    let dt = frame / steps as f32;
    let mut jump = jump;
    let before = w.body.pos;
    for _ in 0..steps {
        let mut vel = dir * speed;
        if w.fly {
            vel.y = (up as i32 - down as i32) as f32 * fly_speed / size;
        } else {
            if w.grounded && std::mem::take(&mut jump) {
                w.vy = (2.0 * GRAVITY * JUMP_HEIGHT).sqrt() / size;
            }
            w.vy -= GRAVITY / size * dt;
            vel.y = w.vy;
        }
        let step = if w.grounded && !w.fly { 1.05 } else { 0.0 };
        let mut body = w.body;
        let contact = {
            let wr: &WorldClient = &w;
            body.travel(with_props(|c| wr.stands_on(c), occupancy), (vel * dt).to_array(), step)
        };
        w.body = body;
        if contact.hit[1] {
            w.vy = 0.0;
        }
        w.grounded = !w.fly
            && (contact.ground || (w.vy <= 0.0 && w.body.on_ground(with_props(|c| w.stands_on(c), occupancy))));
    }
    // walking to a point and stuck against something: give up
    if w.goal.is_some() && dir.length() > 0.1 {
        let moved = Vec2::new(w.body.pos[0] - before[0], w.body.pos[2] - before[2]).length();
        if moved < speed * frame * 0.05 {
            w.goal = None;
        }
    }
    // at a tile's edge that leads on: over it
    w.edge_check();
    if w.body.pos[1] < w.view.confirmed.bounds.lo[1] as f32 - 20.0 {
        w.body.pos = w.spawn;
        w.vy = 0.0;
        w.notice("Back to the start");
    }
    // ---- face where you walk; walk while moving
    let moving = dir.length() > 0.1;
    if w.cam_mode == CamMode::First {
        // in first person you face where you look
        w.yaw = screen_yaw + std::f32::consts::PI;
    } else if moving {
        let goal = dir.x.atan2(dir.z);
        let mut d = goal - w.yaw;
        d = (d + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        w.yaw += d * (1.0 - (-frame * 12.0).exp());
    }
    let walk = moving && !w.fly && av.can_walk;
    if walk != w.moving {
        w.moving = walk;
        av.walking = walk;
    }
    if let Some(e) = w.walker {
        if let Ok(mut t) = walkers.get_mut(e) {
            t.translation = Vec3::from(w.body.pos) * size;
            t.rotation = Quat::from_rotation_y(w.yaw);
        }
    }
    // ---- what you'd build on: the crosshair (behind), the pointer (from above), or with a
    // controller from above, the spot in front of your feet
    let head = Vec3::from(w.body.pos) + Vec3::Y * EYES;
    let in_reach = |h: &Hit| {
        let d2: f32 = (0..3).map(|i| (h.cell[i] as f32 + 0.5 - head[i]).powi(2)).sum();
        d2 <= REACH * REACH
    };
    w.target = if diablo {
        let from_pad = pad_active || cursor_at.is_none();
        if from_pad {
            let ahead = Vec3::new(w.yaw.sin(), 0.0, w.yaw.cos());
            let o = head;
            let d = (ahead * 2.2 - Vec3::Y * 1.6).normalize();
            ray::raycast_with(|c| w.solid_at(c), o.to_array(), d.to_array(), 8.0)
        } else if let Some((o, d)) = w.pick {
            ray::raycast_with(|c| w.solid_at(c), o.to_array(), d.to_array(), 200.0).filter(in_reach)
        } else {
            None
        }
    } else {
        let f = facing(w.cam_yaw, w.cam_pitch);
        let cam = w.cam_at / size;
        // start the ray level with the head, so blocks between the camera and you don't count
        let start = cam + f * (head - cam).dot(f).max(0.0);
        ray::raycast_with(|c| w.solid_at(c), start.to_array(), f.to_array(), REACH + 4.0).filter(in_reach)
    };
    if let Some(h) = w.target {
        if remove {
            w.edit(h.cell, 0);
        } else if place {
            let c = [h.cell[0] + h.normal[0], h.cell[1] + h.normal[1], h.cell[2] + h.normal[2]];
            let b = placed_block(
                editor.as_ref().and_then(|e| e.tool),
                editor.as_ref().and_then(|e| e.block.as_deref()),
                |n| w.block_id(n),
                w.selected(),
            );
            w.edit(c, b);
        } else if pick {
            let b = w.view.get(h.cell);
            w.pick_block(b);
        }
    }
}

/// The block a place click builds. With the editor's Blocks tool, it is the block the page
/// chose, found by name in this world's palette (`lookup`); otherwise, or when the name is not
/// in the palette, it is the hotbar's block.
pub fn placed_block(
    tool: Option<crate::editor::bridge::Tool>,
    chosen: Option<&str>,
    lookup: impl Fn(&str) -> Option<u16>,
    hotbar: u16,
) -> u16 {
    match (tool, chosen) {
        (Some(crate::editor::bridge::Tool::Blocks), Some(name)) => lookup(name).unwrap_or(hotbar),
        _ => hotbar,
    }
}

/// Held break and place, repeating; and when jump was last pressed (twice quickly flies).
#[derive(Default)]
pub struct BuildRepeat {
    held: Option<(crate::input::Action, f64)>,
    last_jump: f64,
    /// how far the view has turned since the button went down (radians)
    turned: f32,
    /// both buttons were up since the game last got the input back (arriving, a menu closing):
    /// a button already held then doesn't count
    armed: bool,
}

impl BuildRepeat {
    /// Turning further than this while holding stops the repeat until the next press.
    pub const TURN: f32 = 0.06;
    pub const FIRST: f64 = 0.3;
    pub const AGAIN: f64 = 0.22;

    /// Given (break, place) held and just pressed this frame, the time, and how far the view
    /// turned this frame: whether to break and whether to place now.
    pub fn step(&mut self, held: (bool, bool), just: (bool, bool), now: f64, turned: f32) -> (bool, bool) {
        use crate::input::Action as A;
        if !self.armed {
            self.armed = !held.0 && !held.1;
            self.held = None;
            return (false, false);
        }
        let mut out = (false, false);
        for (act, h, j) in [(A::Break, held.0, just.0), (A::Place, held.1, just.1)] {
            let fire = if j {
                self.held = Some((act, now + Self::FIRST));
                self.turned = 0.0;
                true
            } else if h && self.held.is_some_and(|(a, _)| a == act) {
                self.turned += turned;
                if self.turned > Self::TURN {
                    // looked away: no more until it's pressed again
                    self.held = None;
                    false
                } else if self.held.is_some_and(|(_, at)| now >= at) {
                    self.held = Some((act, now + Self::AGAIN));
                    true
                } else {
                    false
                }
            } else {
                if !h && self.held.is_some_and(|(a, _)| a == act) {
                    self.held = None;
                }
                false
            };
            if act == A::Break {
                out.0 = fire;
            } else {
                out.1 = fire;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::bridge::Tool;

    #[test]
    fn editor_blocks_tool_places_the_chosen_block() {
        let lookup = |n: &str| (n == "glass").then_some(7u16);
        assert_eq!(placed_block(Some(Tool::Blocks), Some("glass"), lookup, 1), 7);
        // another tool, no choice, or a name not in the palette: the hotbar's block
        assert_eq!(placed_block(Some(Tool::Select), Some("glass"), lookup, 1), 1);
        assert_eq!(placed_block(Some(Tool::Blocks), None, lookup, 1), 1);
        assert_eq!(placed_block(Some(Tool::Blocks), Some("nope"), lookup, 1), 1);
        assert_eq!(placed_block(None, Some("glass"), lookup, 1), 1);
    }
}
