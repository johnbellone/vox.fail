//! vox-client: play Vox. Your character walks into a world: the Void first, and through
//! its doorways to others.
//!
//! The same app runs on the desktop (`vox-client --token <launch code>`) and in the browser
//! (built for WebAssembly by `tools/dev/build_web.sh`, started by the directory's play page). The
//! world itself (blocks, sky, weather, portals, other players, walking and building) is
//! vox-world's; this crate brings the window, the camera, the light, and your character with
//! what you chose to wear.

use std::time::Duration;

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    core_pipeline::tonemapping::Tonemapping,
    gltf::Gltf,
    post_process::bloom::Bloom,
    prelude::*,
    camera::Hdr,
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};
use serde::Deserialize;
use vox_world::{
    KeyLight, Level, LocalAvatar, WorldCamera, WorldRender, blocks, client as world, outdoors, portals, pose,
};

/// How to start: which world, who you are and what you look like.
#[derive(Deserialize, Clone, Debug, Default, Resource)]
pub struct Start {
    /// the world server (`ws://host:7780/ws`)
    pub url: String,
    /// a sign-in token from the directory
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub name: String,
    /// your character's model, under the assets folder
    #[serde(default)]
    pub model: String,
    /// what you wear, by wardrobe item id
    #[serde(default)]
    pub wear: Vec<String>,
    /// the page's canvas (browser only)
    #[serde(default)]
    pub canvas: Option<String>,
    /// watch the world instead of playing: no sign-in, no avatar, no controls or HUD, a
    /// drifting camera (the front page's live view)
    #[serde(default)]
    pub watch: bool,
    /// edit the world (the in-world editor, for those the world lets change it): the page
    /// sends commands and hears events through `editor_command` and `window.vox_editor`.
    /// Browser only: the desktop client ignores it, since there is no page to talk to
    #[serde(default)]
    pub edit: bool,
}

impl Start {
    /// Fill in what a token says (name, character) where nothing else did.
    pub fn from_token(mut self) -> Start {
        if let Some(c) = self.token.as_deref().and_then(vox_core::auth::peek) {
            if self.name.is_empty() {
                self.name = c.name;
            }
            if self.model.is_empty() {
                self.model = c.character.model;
                self.wear = c.character.wear;
            }
        }
        if self.model.is_empty() {
            self.model = "models/reporter.glb".into();
        }
        self
    }
}

/// Your character, and its clips.
#[derive(Resource, Default)]
struct Me {
    graph: Option<Handle<AnimationGraph>>,
    idle: Option<AnimationNodeIndex>,
    walk: Option<AnimationNodeIndex>,
    player: Option<Entity>,
    walking: bool,
    gltf: Option<Handle<Gltf>>,
    spawned: bool,
    dressed: bool,
}

/// The app, ready to run.
pub fn app(start: Start, assets: String) -> App {
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    let start_edit = start.edit;
    let level = if cfg!(target_arch = "wasm32") { Level::Medium } else { Level::High };
    // screenshots at a set size (`VOX_SCREENSHOT_SIZE=960x540`), so pictures stay comparable
    let mut window = bevy::window::WindowResolution::default();
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((w, h)) = std::env::var("VOX_SCREENSHOT_SIZE").ok().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.trim().parse::<u32>().ok()?.clamp(320, 3840), h.trim().parse::<u32>().ok()?.clamp(240, 2160)))
    }) {
        window = bevy::window::WindowResolution::new(w, h);
    }
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Vox".into(),
                    resolution: window,
                    canvas: start.canvas.clone(),
                    fit_canvas_to_parent: true,
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin { file_path: assets, ..default() }),
    )
    .add_plugins((
        MaterialPlugin::<blocks::BlockMaterial>::default(),
        MaterialPlugin::<blocks::WaterMaterial>::default(),
        MaterialPlugin::<blocks::FlameMaterial>::default(),
        MaterialPlugin::<outdoors::SkyMaterial>::default(),
        MaterialPlugin::<outdoors::FloorMaterial>::default(),
        MaterialPlugin::<outdoors::RainMaterial>::default(),
        MaterialPlugin::<portals::PortalMaterial>::default(),
    ))
    .add_plugins((FrameTimeDiagnosticsPlugin::default(), vox_world::input::InputPlugin, vox_world::voice::plugin::VoicePlugin, vox_world::hud::HudPlugin, vox_world::menu::MenuPlugin, vox_world::props::PropPlugin))
    .insert_resource(ClearColor(Color::srgb(0.93, 0.94, 0.96)))
    .insert_resource(
        world::WorldClient::new(Some(start.url.clone()), start.name.clone())
            .with_token(start.token.clone())
            .watching(start.watch),
    )
    .insert_resource(WorldRender { level, shadows: true, outdoors: false, env_maps: !cfg!(target_arch = "wasm32") })
    .insert_resource(LocalAvatar { model: start.model.clone(), fallback: "models/blue.glb".into(), ..default() })
    // watching: nobody to spawn
    .insert_resource(Me { spawned: start.watch, ..default() })
    .insert_resource(start)

    .init_resource::<outdoors::Outdoors>()
    .init_resource::<portals::Portals>()
    .init_resource::<vox_world::entities::WorldEntities>()
    .add_systems(Startup, (blocks::setup, setup, (spawn_fps, spawn_fade).run_if(playing)))
    .add_systems(
        Update,
        (load_me, walk, world::receive, world::attach, world::controls, world::remesh, world::hud).chain(),
    )
    .add_systems(Update, (world::camera, world::weather).after(world::controls))
    .add_systems(Update, (outdoors::update, outdoors::weather_look, outdoors::relief_to_blocks).chain().after(world::camera))
    .add_systems(Update, portals::update.after(world::camera).after(world::receive))
    .add_systems(Update, (dress, pose::load_remotes, report, (update_fps, fade).run_if(playing)))
    .add_systems(
        PostUpdate,
        (world::send_pose, world::apply_others).chain().before(bevy::transform::TransformSystems::Propagate),
    );
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Update, web::fetch_wanted);
    // the editor talks to a page, so it exists only in the browser: its commands come in before
    // the editor acts, its events go out after it
    #[cfg(target_arch = "wasm32")]
    if start_edit {
        app.add_plugins(vox_world::editor::EditorPlugin);
        app.add_systems(Update, (web::take_commands.before(vox_world::editor::EditorSystems), web::send_events.after(vox_world::editor::EditorSystems)));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(path) = std::env::var("VOX_SCREENSHOT") {
        app.insert_resource(ShotAt { path, after: std::env::var("VOX_SCREENSHOT_AFTER").ok().and_then(|v| v.parse().ok()).unwrap_or(6.0), taken: None })
            .add_systems(Update, screenshot);
    }
    app
}

/// For checking the screen without looking: `VOX_SCREENSHOT=shot.png` saves a picture of the
/// window `VOX_SCREENSHOT_AFTER` seconds (6) after the world is ready and drawn, then quits.
/// `VOX_SCREENSHOT_DO="time:17;look:200:-10;camera:behind"` runs those `world:` commands first
/// (the control page's, without `world:`), `VOX_SCREENSHOT_SIZE=960x540` sets the window's
/// size, and `VOX_SCREENSHOT_CLEAN=1` leaves the HUD out: the
/// README's pictures are made this way (`tools/dev/screenshots.sh`).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
struct ShotAt {
    path: String,
    after: f64,
    taken: Option<f64>,
}

#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::too_many_arguments)]
fn screenshot(
    mut commands: Commands,
    mut w: ResMut<world::WorldClient>,
    ui: Query<Entity, (With<Node>, Without<ChildOf>)>,
    mut gizmos: ResMut<GizmoConfigStore>,
    mut shot: ResMut<ShotAt>,
    mut ready_at: Local<Option<f64>>,
    mut exit: MessageWriter<AppExit>,
    mut menus: ResMut<vox_world::menu::Menus>,
    mut wheel: ResMut<vox_world::hud::QuickWheel>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    use vox_world::menu::Screen;
    let now = w.now();
    // wait for the world to be drawn, not just to have arrived
    if !w.settled() && ready_at.is_none() {
        return;
    }
    if std::env::var("VOX_SCREENSHOT_CLEAN").is_ok_and(|v| !v.is_empty() && v != "0") {
        // every piece of the HUD hangs off a root node: take them all away; and the outline
        // round the block in sight (a gizmo)
        for e in &ui {
            commands.entity(e).despawn();
        }
        gizmos.config_mut::<DefaultGizmoConfigGroup>().0.enabled = false;
    }
    let ready = *ready_at.get_or_insert_with(|| {
        if let Ok(list) = std::env::var("VOX_SCREENSHOT_DO") {
            w.cmds.extend(list.split(';').map(str::trim).filter(|c| !c.is_empty()).map(String::from));
        }
        // VOX_SCREENSHOT_MENU=pause|blocks|controls|settings|wheel: with that menu open
        if std::env::var("VOX_SCREENSHOT_MENU").as_deref() == Ok("wheel") {
            wheel.scripted = Some(Vec2::new(70.0, 70.0));
        }
        let screen = match std::env::var("VOX_SCREENSHOT_MENU").as_deref() {
            Ok("pause") => Some(Screen::Pause),
            Ok("blocks") => Some(Screen::Inventory),
            Ok("controls") => Some(Screen::Controls),
            Ok("settings") => Some(Screen::Settings),
            _ => None,
        };
        if let Some(s) = screen {
            menus.open(s);
        }
        now
    });
    match shot.taken {
        None if now - ready >= shot.after => {
            commands.spawn(Screenshot::primary_window()).observe(save_to_disk(shot.path.clone()));
            shot.taken = Some(now);
        }
        Some(t) if now - t > 2.0 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// Playing, not watching: the frame rate and the portal sweep are for players.
fn playing(start: Res<Start>) -> bool {
    !start.watch
}

#[derive(Component)]
struct FadeCover;
/// One streak of the sweep: where it sits, how fast it runs and where in the loop it starts.
#[derive(Component)]
struct Streak {
    speed: f32,
    phase: f32,
}

const STREAKS: usize = 22;

/// A screen-wide wash with streaks racing across it, up while a world is loading (walking
/// through a portal, or the first connect) and gone once the new world is ready.
fn spawn_fade(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.8, 0.85, 1.0, 1.0)),
            GlobalZIndex(1000),
            FadeCover,
        ))
        .with_children(|p| {
            for i in 0..STREAKS {
                let f = i as f32 / STREAKS as f32;
                // a spread of heights, lengths and speeds, the same every time
                let h = 1.5 + 4.0 * ((i * 7 % 5) as f32 / 4.0);
                let len = 30.0 + 50.0 * ((i * 3 % 7) as f32 / 6.0);
                p.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Percent(f * 100.0),
                        left: Val::Percent(-100.0),
                        width: Val::Percent(len),
                        height: Val::Percent(h),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.0)),
                    Streak { speed: 1.6 + 1.4 * ((i * 5 % 9) as f32 / 8.0), phase: (i * 13 % 11) as f32 / 11.0 },
                ));
            }
        });
}

fn fade(
    time: Res<Time>,
    w: Res<world::WorldClient>,
    mut level: Local<Option<f32>>,
    mut cover: Query<(&mut BackgroundColor, &mut Visibility), (With<FadeCover>, Without<Streak>)>,
    mut streaks: Query<(&Streak, &mut Node, &mut BackgroundColor), Without<FadeCover>>,
) {
    let a = level.get_or_insert(1.0);
    let dt = time.delta_secs();
    *a = if w.ready { (*a - dt / 0.5).max(0.0) } else { (*a + dt / 0.2).min(1.0) };
    let a = *a;
    for (mut bg, mut vis) in &mut cover {
        bg.0 = Color::srgba(0.8, 0.85, 1.0, a * 0.8);
        *vis = if a > 0.0 { Visibility::Inherited } else { Visibility::Hidden };
    }
    let t = time.elapsed_secs();
    for (s, mut node, mut bg) in &mut streaks {
        // each streak runs left to right and starts over past the edge
        let x = (t * s.speed + s.phase).fract();
        node.left = Val::Percent(-100.0 + x * 240.0);
        bg.0 = Color::srgba(1.0, 1.0, 1.0, a * 0.55);
    }
}

#[derive(Component)]
struct FpsText;

/// The frame rate, top right.
fn spawn_fps(mut commands: Commands) {
    commands.spawn((
        Text::new("-- fps"),
        TextFont { font_size: bevy::text::FontSize::Px(14.0), ..default() },
        TextColor(Color::srgb(1.0, 0.55, 0.1)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(8.0),
            right: Val::Px(8.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.55)),
        FpsText,
    ));
}

fn update_fps(diagnostics: Res<DiagnosticsStore>, mut q: Query<&mut Text, With<FpsText>>) {
    let Some(fps) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()) else {
        return;
    };
    for mut t in &mut q {
        t.0 = format!("{fps:.0} fps");
    }
}

fn setup(mut commands: Commands, render: Res<WorldRender>) {
    let mut cam = commands.spawn((
        Camera3d::default(),
        WorldCamera,
        Tonemapping::TonyMcMapface,
        Transform::from_xyz(0.0, 6.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    if render.level >= Level::Medium {
        cam.insert((Hdr, Bloom::NATURAL));
    }
    commands.spawn((
        DirectionalLight { illuminance: 8000.0, shadow_maps_enabled: render.shadows, ..default() },
        Transform::from_xyz(4.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        KeyLight,
    ));
}

/// Load your character and its clips; spawn it once loaded.
fn load_me(
    mut me: ResMut<Me>,
    mut local: ResMut<LocalAvatar>,
    assets: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut commands: Commands,
) {
    if me.spawned {
        return;
    }
    let h = me.gltf.get_or_insert_with(|| assets.load::<Gltf>(local.model.clone())).clone();
    let Some(gltf) = gltfs.get(&h) else { return };
    let Some(scene) = gltf.default_scene.clone().or_else(|| gltf.scenes.first().cloned()) else { return };
    let mut graph = AnimationGraph::new();
    let clip = |name: &str, graph: &mut AnimationGraph| {
        gltf.named_animations.get(name).map(|c| graph.add_clip(c.clone(), 1.0, graph.root))
    };
    me.idle = clip("Idle", &mut graph);
    me.walk = clip("Walk", &mut graph);
    me.graph = Some(graphs.add(graph));
    let root = commands.spawn((WorldAssetRoot(scene), Transform::default(), Name::new("me"))).observe(on_ready).id();
    local.root = Some(root);
    local.gltf = Some(h);
    local.can_walk = me.walk.is_some();
    me.spawned = true;
}

/// Your character is in the world: start its animation; its bare head (if it has one) stays
/// hidden unless it wears a hat.
fn on_ready(
    ready: On<WorldInstanceReady>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut players: Query<&mut AnimationPlayer>,
    mut me: ResMut<Me>,
    mut local: ResMut<LocalAvatar>,
    mut commands: Commands,
) {
    let Some(graph) = me.graph.clone() else { return };
    for e in children.iter_descendants(ready.entity) {
        if let Ok(mut player) = players.get_mut(e) {
            let mut t = AnimationTransitions::new();
            if let Some(idle) = me.idle {
                t.play(&mut player, idle, Duration::ZERO).repeat();
            }
            commands.entity(e).insert((AnimationGraphHandle(graph.clone()), t));
            me.player = Some(e);
        }
        if names.get(e).is_ok_and(|n| n.as_str() == "Head_bare") {
            commands.entity(e).insert(Visibility::Hidden);
        }
    }
    local.ready = me.player.is_some();
}

/// Walk or stand, as the world says.
fn walk(
    mut me: ResMut<Me>,
    local: Res<LocalAvatar>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    if local.walking == me.walking {
        return;
    }
    let Some(e) = me.player else { return };
    let Ok((mut p, mut t)) = players.get_mut(e) else { return };
    me.walking = local.walking;
    let clip = if me.walking { me.walk } else { me.idle };
    if let Some(c) = clip {
        t.play(&mut p, c, Duration::from_millis(180)).repeat();
    }
}

/// Put on what you chose to wear, once your character is there: hats and face pieces on the
/// head, a held thing in the right hand (data/head_fit.json says where on each head).
fn dress(
    mut me: ResMut<Me>,
    start: Res<Start>,
    local: Res<LocalAvatar>,
    children: Query<&Children>,
    names: Query<&Name>,
    assets: Res<AssetServer>,
    mut vis: Query<&mut Visibility>,
    mut commands: Commands,
) {
    if me.dressed || !local.ready {
        return;
    }
    let Some(root) = local.root else { return };
    me.dressed = true;
    let file = local.model.rsplit('/').next().unwrap_or("").to_string();
    let json = |p: &str| {
        vox_world::files::read_string(&vox_world::files::base().join(p))
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .unwrap_or_default()
    };
    let (fits, bare, wardrobe) = (json("data/head_fit.json"), json("data/bare_heads.json"), json("data/wardrobe.json"));
    let fit = |k: &str, d: f32| fits.get(&file).and_then(|f| f.get(k)).and_then(|v| v.as_f64()).map_or(d, |v| v as f32);
    let (top, width, front, eye_y, mouth_y) = (fit("top", 0.62), fit("width", 0.7), fit("front", 0.31), fit("eye_y", 0.25), fit("mouth_y", 0.07));
    let head_scale = (width / 0.62).clamp(0.85, 1.35);
    let mut head = None;
    let mut hand = None;
    let mut bare_head = None;
    for e in children.iter_descendants(root) {
        match names.get(e).map(|n| n.as_str()) {
            Ok("Head") => head = Some(e),
            Ok("LowerArm_R") => hand = Some(e),
            Ok("Head_bare") => bare_head = Some(e),
            _ => {}
        }
    }
    let items = wardrobe.get("items").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    for id in &start.wear {
        let Some(it) = items.iter().find(|i| i.get("id").and_then(|v| v.as_str()) == Some(id)) else { continue };
        let slot = it.get("slot").and_then(|v| v.as_str()).unwrap_or("");
        let Some(path) = it.get("file").and_then(|v| v.as_str()) else { continue };
        let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(path.to_string()));
        let (parent, at, scale) = match slot {
            "hat" => {
                // a character with its own cap takes it off for a hat
                let bare_top = bare.get(&file).and_then(|v| v.as_f64()).map(|v| v as f32);
                if let (Some(t), Some(b)) = (bare_top, bare_head) {
                    if let Ok(mut v) = vis.get_mut(b) {
                        *v = Visibility::Inherited;
                    }
                    let _ = t;
                }
                (head, Vec3::new(0.0, bare_top.filter(|_| bare_head.is_some()).unwrap_or(top) - 0.01, 0.0), head_scale)
            }
            "face" => {
                let y = match id.as_str() {
                    "moustache" => mouth_y + 0.06,
                    "clown" => (eye_y + mouth_y) / 2.0,
                    _ => eye_y,
                };
                (head, Vec3::new(0.0, y, front + 0.005), head_scale)
            }
            _ => (hand, Vec3::new(0.0, -0.25, 0.05), 1.0),
        };
        let Some(p) = parent else { continue };
        commands.spawn((WorldAssetRoot(scene), Transform::from_translation(at).with_scale(Vec3::splat(scale)), ChildOf(p)));
    }
    // under a hat, the capped head gives way to the bare one
    if start.wear.iter().any(|id| items.iter().any(|i| i.get("id").and_then(|v| v.as_str()) == Some(id) && i.get("slot").and_then(|v| v.as_str()) == Some("hat"))) && bare_head.is_some() && bare.get(&file).is_some() {
        if let Some(h) = head {
            for e in children.iter_descendants(h) {
                if names.get(e).is_ok_and(|n| n.as_str() == "Head_mesh" || n.as_str() == "Head_mesh.Voxel") {
                    commands.entity(e).insert(Visibility::Hidden);
                }
            }
        }
    }
}

/// Tell the page how it's going (the browser's loading screen), and when you're in.
fn report(w: Res<world::WorldClient>, mut said: Local<String>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let s = if w.ready { "ready".to_string() } else { w.status.clone() };
    if *said == s {
        return;
    }
    *said = s.clone();
    if w.ready {
        *done = true;
    }
    #[cfg(target_arch = "wasm32")]
    web::tell(&s);
    #[cfg(not(target_arch = "wasm32"))]
    if !s.is_empty() {
        vox_world::note!("[client] {s}");
    }
}

/// Most editor commands queued between two frames.
#[cfg(any(target_arch = "wasm32", test))]
const MAX_QUEUED: usize = 256;

/// Most bytes of editor commands queued between two frames: a page cannot pile up a megabyte of
/// work (or a few huge commands) in one frame.
#[cfg(any(target_arch = "wasm32", test))]
const MAX_QUEUED_BYTES: usize = 1 << 20;

/// Whether a command of `size` bytes may join the queue that holds `count` commands of
/// `bytes` bytes in all.
#[cfg(any(target_arch = "wasm32", test))]
fn queue_admits(count: usize, bytes: usize, size: usize) -> bool {
    count < MAX_QUEUED && bytes.saturating_add(size) <= MAX_QUEUED_BYTES
}

#[cfg(target_arch = "wasm32")]
mod web {
    //! The browser: fetch the game's files before starting, and others as they're wanted.

    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    use bevy::prelude::ResMut;
    use super::queue_admits;
    use vox_world::editor::{EditorInbox, EditorOutbox, bridge::{self, Command, Event}};
    use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
    use wasm_bindgen_futures::{JsFuture, spawn_local};

    pub async fn fetch(path: &str) -> Option<Vec<u8>> {
        let win = web_sys::window()?;
        let resp = JsFuture::from(win.fetch_with_str(&format!("/{path}"))).await.ok()?;
        let resp: web_sys::Response = resp.dyn_into().ok()?;
        if !resp.ok() {
            return None;
        }
        let buf = JsFuture::from(resp.array_buffer().ok()?).await.ok()?;
        Some(js_sys::Uint8Array::new(&buf).to_vec())
    }

    /// Fetch `path` into vox-world's files.
    pub async fn get(path: &str) -> bool {
        match fetch(path).await {
            Some(b) => {
                vox_world::files::put(path, b);
                true
            }
            None => false,
        }
    }

    /// Files the world asked for (a portal's level): fetch them in the background.
    pub fn fetch_wanted() {
        for p in vox_world::files::take_wanted() {
            spawn_local(async move {
                if !get(&p).await {
                    vox_world::files::put(&p, Vec::new());
                }
            });
        }
    }

    thread_local! {
        /// commands from the page, parsed, waiting for the app's next frame
        static COMMANDS: RefCell<Vec<Command>> = const { RefCell::new(Vec::new()) };
        /// the bytes of the commands in COMMANDS (their JSON), for the byte cap
        static QUEUED_BYTES: Cell<usize> = const { Cell::new(0) };
        /// whether this page started the app in edit mode (its commands are refused otherwise)
        static EDITING: Cell<bool> = const { Cell::new(false) };
        /// events the page has not heard yet because `window.vox_editor` did not exist
        static UNHEARD: RefCell<VecDeque<Event>> = const { RefCell::new(VecDeque::new()) };
        /// events are being handed to the page now (a handler that calls back must not reorder)
        static HANDING: Cell<bool> = const { Cell::new(false) };
    }

    /// Most events kept for a page that has not installed `vox_editor` yet; the oldest go first.
    const MAX_UNHEARD: usize = 1024;

    /// The page's `vox_editor(json)`, if it has installed one.
    fn editor_handler() -> Option<js_sys::Function> {
        let win = web_sys::window()?;
        js_sys::Reflect::get(&win, &JsValue::from_str("vox_editor")).ok()?.dyn_into::<js_sys::Function>().ok()
    }

    /// Hands the page the events it has not heard, in order, once it has `vox_editor`.
    fn flush_editor() {
        if HANDING.with(Cell::get) {
            return;
        }
        let Some(func) = editor_handler() else { return };
        HANDING.with(|h| h.set(true));
        while let Some(e) = UNHEARD.with(|q| q.borrow_mut().pop_front()) {
            let _ = func.call1(&JsValue::NULL, &JsValue::from_str(&bridge::event_json(&e)));
        }
        HANDING.with(|h| h.set(false));
    }

    /// Tell the page an editor event. Until the page has a `vox_editor` the events wait (the
    /// newest 1024), so the first ones (Ready, the snapshot) are not lost to a page that loads
    /// its editor a moment after the client starts.
    fn tell_editor(e: Event) {
        UNHEARD.with(|q| {
            let mut q = q.borrow_mut();
            if q.len() >= MAX_UNHEARD {
                q.pop_front();
            }
            q.push_back(e);
        });
        flush_editor();
    }

    /// A command from the page, as JSON. Queued for the next frame when this app is editing;
    /// refused to the page (as a `Refused` event, with its `ref` when that can be read)
    /// otherwise, when it can't be read, or when too many are queued already.
    #[wasm_bindgen]
    pub fn editor_command(json: String) {
        let refuse = |why: &str, text: String| {
            tell_editor(Event::Refused { id: 0, r#ref: bridge::ref_of(&json), why: why.into(), text })
        };
        if !EDITING.with(Cell::get) {
            return refuse("NotEditing", "this client didn't start in edit mode".into());
        }
        let command = match bridge::parse_command(&json) {
            Ok(c) => c,
            Err(text) => return refuse("Invalid", text),
        };
        let size = json.len();
        let queued = COMMANDS.with(|q| {
            let mut q = q.borrow_mut();
            let bytes = QUEUED_BYTES.with(Cell::get);
            if !queue_admits(q.len(), bytes, size) {
                false
            } else {
                q.push(command);
                QUEUED_BYTES.with(|b| b.set(bytes + size));
                true
            }
        });
        if !queued {
            refuse("TooFast", "too many editor commands at once".into());
        }
    }

    /// Moves the page's queued commands into the editor's inbox.
    pub fn take_commands(mut inbox: ResMut<EditorInbox>) {
        COMMANDS.with(|q| inbox.0.append(&mut q.borrow_mut()));
        QUEUED_BYTES.with(|b| b.set(0));
    }

    /// Sends the editor's events to the page, in order. An event is held only while the page has
    /// no `vox_editor` (it is handed over once one appears); a handler that throws drops that
    /// one event and the rest still go. Past 1024 held events the oldest are dropped, which a
    /// page avoids by installing `vox_editor` before it calls `start`.
    pub fn send_events(mut outbox: ResMut<EditorOutbox>) {
        for e in outbox.0.drain(..) {
            tell_editor(e);
        }
        flush_editor();
    }

    /// Tell the page: `vox_status(text)`, or `vox_ready()` once in.
    pub fn tell(s: &str) {
        let Some(win) = web_sys::window() else { return };
        let (f, arg) = if s == "ready" { ("vox_ready", None) } else { ("vox_status", Some(s)) };
        if let Ok(func) = js_sys::Reflect::get(&win, &JsValue::from_str(f)) {
            if let Some(func) = func.dyn_ref::<js_sys::Function>() {
                let _ = match arg {
                    Some(a) => func.call1(&JsValue::NULL, &JsValue::from_str(a)),
                    None => func.call0(&JsValue::NULL),
                };
            }
        }
    }

    /// Start in the browser: `start(JSON.stringify({url, token, name, model, wear, canvas}))`.
    #[wasm_bindgen]
    pub fn start(config: String) {
        let start: super::Start = match serde_json::from_str(&config) {
            Ok(s) => s,
            Err(e) => {
                tell(&format!("Couldn't start: {e}"));
                return;
            }
        };
        let start = start.from_token();
        EDITING.with(|e| e.set(start.edit));
        spawn_local(async move {
            tell("Fetching the world's blocks…");
            let _ = get("data/blocks.json").await;
            for f in ["data/head_fit.json", "data/bare_heads.json", "data/wardrobe.json"] {
                let _ = get(f).await;
            }
            let tiles = vox_world::blocks::Palette::load(std::path::Path::new("data/blocks.json")).map(|p| p.tiles()).unwrap_or_default();
            let jobs: Vec<_> = tiles
                .iter()
                .flat_map(|t| [format!("assets/blocks/tiles/{t}.png"), format!("assets/blocks/tiles/{t}.height.png")])
                .collect();
            for chunk in jobs.chunks(16) {
                let fs: Vec<_> = chunk.iter().map(|p| get(p)).collect();
                for f in fs {
                    let _ = f.await;
                }
            }
            tell("Starting…");
            super::app(start, "assets".into()).run();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vox_core::auth::{self, Character, Claims};

    #[test]
    fn the_editor_queue_is_capped_by_count_and_bytes() {
        assert!(queue_admits(0, 0, 100));
        assert!(!queue_admits(MAX_QUEUED, 0, 1));
        assert!(queue_admits(MAX_QUEUED - 1, 0, 1));
        assert!(queue_admits(0, MAX_QUEUED_BYTES - 10, 10));
        assert!(!queue_admits(0, MAX_QUEUED_BYTES - 10, 11));
        assert!(!queue_admits(0, usize::MAX, 1));
    }

    #[test]
    fn a_token_says_who_you_are() {
        let claims = Claims {
            account: 1,
            name: "Sam".into(),
            character: Character { model: "models/judge.glb".into(), wear: vec!["crown".into()] },
            issued: 0,
            expires: 10,
        };
        let token = auth::sign(&claims, &auth::key_from_seed([1; 32]));
        let s = Start { url: "ws://x/ws".into(), token: Some(token), ..default() }.from_token();
        assert_eq!((s.name.as_str(), s.model.as_str()), ("Sam", "models/judge.glb"));
        assert_eq!(s.wear, vec!["crown"]);
        // nothing to go on: someone to be anyway
        let s = Start { url: "ws://x/ws".into(), ..default() }.from_token();
        assert_eq!(s.model, "models/reporter.glb");
    }
}
