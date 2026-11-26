//! Smooth starts and switches:
//! - a loading card ("Loading Dog...") instead of an empty frame while a character loads and the
//!   graphics warm up, in the window and the virtual webcam alike;
//! - once the first character is up, every other character, hat and set loads quietly in the
//!   background, so switching is instant;
//! - switching microphones from the setup page without restarting.

use bevy::{gltf::Gltf, gltf::GltfAssetLabel, prelude::*, world_serialization::WorldAsset};

#[derive(Resource, Default)]
pub struct Preload {
    handles: Vec<UntypedHandle>,
    done: bool,
}

#[derive(Component)]
pub struct LoadingCard;

pub fn spawn_card(
    stages: Res<crate::stage::Stages>,
    assets: Res<AssetServer>,
    mut commands: Commands,
) {
    let font: Handle<Font> = assets.load("fonts/PressStart2P-Regular.ttf");
    for v in stages.views.iter() {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ChildOf(v.root),
            GlobalZIndex(50),
            LoadingCard,
            children![(
                Node {
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(14.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.06, 0.07, 0.12, 0.85)),
                Text::new("Loading..."),
                TextFont {
                    font: font.clone().into(),
                    font_size: bevy::text::FontSize::Px(16.0),
                    ..default()
                },
                TextColor(Color::srgb_u8(242, 182, 50)),
            )],
        ));
    }
}

/// Shows the card while the character loads, and for a moment after (first frames can stutter
/// while the graphics card compiles its shaders).
pub fn update_card(
    av: Res<crate::Avatar>,
    time: Res<Time>,
    mut ready_at: Local<Option<f32>>,
    mut cards: Query<(&mut Visibility, &Children), With<LoadingCard>>,
    mut texts: Query<&mut Text>,
) {
    let now = time.elapsed_secs();
    let ready = av.player.is_some();
    match (ready, *ready_at) {
        (true, None) => *ready_at = Some(now),
        (false, Some(_)) => *ready_at = None,
        _ => {}
    }
    let show = !ready || ready_at.is_some_and(|t| now - t < 0.7);
    let line = format!("Loading {}...", av.models[av.model].0);
    for (mut v, kids) in &mut cards {
        let want = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
        if show {
            for k in kids.iter() {
                if let Ok(mut t) = texts.get_mut(k) {
                    if t.0 != line {
                        t.0 = line.clone();
                    }
                }
            }
        }
    }
}

/// After the first character is on screen, load everything else in the background.
pub fn preload(av: Res<crate::Avatar>, mut pre: ResMut<Preload>, assets: Res<AssetServer>) {
    if pre.done || av.player.is_none() {
        return;
    }
    pre.done = true;
    for (_, path) in av.models.iter() {
        pre.handles
            .push(assets.load::<Gltf>(path.clone()).untyped());
    }
    for item in crate::wardrobe::ITEMS {
        pre.handles.push(
            assets
                .load::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(item.file))
                .untyped(),
        );
    }
    for s in crate::show::SETS {
        pre.handles.push(
            assets
                .load::<WorldAsset>(GltfAssetLabel::Scene(0).from_asset(format!("sets/{s}.glb")))
                .untyped(),
        );
    }
    crate::note!(
        "[startup] preloading {} characters, props and sets",
        pre.handles.len()
    );
}

/// A new microphone was picked on the setup page: close the old one, open the new one, and
/// measure the room again.
pub fn switch_mic(
    mut av: ResMut<crate::Avatar>,
    mut mic: ResMut<crate::Mic>,
    mut lip: ResMut<crate::Lip>,
    time: Res<Time>,
) {
    let Some(name) = av.mic_switch.take() else {
        return;
    };
    mic.0.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    *mic = crate::start_mic(Some(name).filter(|n| !n.is_empty()));
    lip.floor = crate::lipsync::NoiseFloor::new();
    lip.calibrate_until = time.elapsed_secs() + 1.5;
    lip.last_frames = 0;
}
