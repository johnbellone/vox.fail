//! The app's systems, as plugins by pipeline. Each `add_systems` call is as it was in
//! `main`, so every chain and every before/after keeps its order: several chains run systems
//! from different folders in a set order (the startup chain, the per-frame chains), so the
//! plugins follow those pipelines rather than the folders.

#[allow(unused_imports)]
use crate::*;

/// All of them.
pub struct AvatarPlugins;

impl PluginGroup for AvatarPlugins {
    fn build(self) -> bevy::app::PluginGroupBuilder {
        bevy::app::PluginGroupBuilder::start::<Self>().add(Setup).add(Frame).add(Motion).add(Network).add(WorldGlue)
    }
}

/// Startup: the scene, the model, the webcam, clips, stages, bubbles, poll labels, the
/// start card, chat, sound, lighting, particles and blocks, in that order.
pub struct Setup;

impl Plugin for Setup {
    fn build(&self, app: &mut App) {
        app
        .add_systems(
            Startup,
            (
                setup_scene,
                load_model,
                webcam::setup,
                clips::setup,
                stage::setup,
                bubbles::spawn,
                poll::spawn_labels,
                startup::spawn_card,
                chat::setup,
                fx::setup,
                lighting::setup,
                particles::setup,
                blocks::setup,
            )
                .chain(),
        );
    }
}

/// Every frame: input, commands, the guest, lip sync, the body and camera, the webcam and
/// clips, the HUD and published state; then, after the camera, everything on stage (throws,
/// dress-up, Twitch moments, polls, voice effects, the co-host, the show, puppets, hits, sound,
/// bubbles); the fixed-rate steps; face capture, sets, idle life, the crowd and snapshots.
pub struct Frame;

impl Plugin for Frame {
    fn build(&self, app: &mut App) {
        app
        .add_systems(
            Update,
            (
                handle_input,
                hotkeys::pump,
                handle_remote,
                startup::switch_mic,
                guest::update,
                update_lipsync,
                build_graph_when_loaded,
                drive_body,
                update_camera,
                webcam::follow,
                clips::update,
                stage::update,
                update_hud,
                publish_state,
                startup::update_card,
                startup::preload,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                chat::launch,
                wardrobe::tag_heads,
                wardrobe::update,
                wardrobe::swap_heads,
                events::update,
                poll::update,
                voicefx::update,
                cohost::update,
                show::update,
                puppet::load,
                puppet::animate,
                chat::fly,
                apply_hits,
                fx::update,
                bubbles::poll,
                bubbles::layout,
            )
                .chain()
                .after(update_camera),
        )
        .add_systems(
            FixedUpdate,
            (particles::step, crowd::step, fx::step, chat::fade),
        )
        .add_systems(Update, facecap::update)
        .add_systems(Update, sets::build)
        .add_systems(
            Update,
            (
                idle_life,
                jiggle::attach,
                crowd::update,
                crowd::signs.after(crowd::update),
            ),
        )
        .add_systems(Update, snapshot::run.after(startup::update_card));
    }
}

/// After animation: the face, the guest's and the puppets' faces, bevels, secondary
/// motion, and the studio's blur.
pub struct Motion;

impl Plugin for Motion {
    fn build(&self, app: &mut App) {
        app
        .add_systems(
            PostUpdate,
            (drive_face, guest::drive_face, puppet::faces)
                .after(AnimationSystems)
                .before(TransformSystems::Propagate),
        )
        .add_systems(PostUpdate, bevel::apply)
        .add_systems(PostUpdate, lighting::blur.after(TransformSystems::Propagate))
        .add_systems(
            PostUpdate,
            jiggle::update
                .after(drive_face)
                .after(AnimationSystems)
                .before(TransformSystems::Propagate),
        );
    }
}

/// Shows over the network: receiving, cues, finding bodies, holding still, voices, the
/// stage; and after the faces, sending, applying and placing the guest.
pub struct Network;

impl Plugin for Network {
    fn build(&self, app: &mut App) {
        app
        .add_systems(
            Update,
            (net::receive, particles::take_cues, net::take_cues, lighting::update).chain(),
        )
        .add_systems(
            Update,
            (
                net::find_body,
                net::hold_still,
                net::load_remotes,
                net::voice,
                net::stage.after(cohost::update),
            ),
        )
        .add_systems(
            PostUpdate,
            (net::send, net::apply, net::place_guest)
                .chain()
                .after(drive_face)
                .after(AnimationSystems)
                .before(jiggle::update)
                .before(TransformSystems::Propagate),
        );
    }
}

/// A Vox world inside the app (`--world`): the world client's systems, its camera,
/// weather, sky and portals, and its input actions.
pub struct WorldGlue;

impl Plugin for WorldGlue {
    fn build(&self, app: &mut App) {
        app
        .init_resource::<vox_world::WorldRender>()
        .init_resource::<vox_world::LocalAvatar>()
        // walking and building in a world read the same actions as the player client
        .add_plugins(vox_world::input::InputPlugin)
        .add_systems(
            Update,
            (world_in, world::receive, world::attach, world::controls, world_out, world::remesh, world::hud)
                .chain()
                .before(update_camera),
        )
        .add_systems(Update, world::camera.after(update_camera).after(net::receive))
        .add_systems(Update, world::weather.after(world::receive))
        .add_systems(Update, (outdoors::update, outdoors::weather_look, outdoors::relief_to_blocks, world_out).chain().after(world::camera).after(lighting::update))
        .add_systems(Update, portals::update.after(world::camera).after(world::receive))
        .init_resource::<portals::Portals>()
        .add_systems(
            PostUpdate,
            (world::send_pose, world::apply_others)
                .chain()
                .after(net::place_guest)
                .before(jiggle::update)
                .before(TransformSystems::Propagate),
        );
    }
}
