//! Between the app and a Vox world (`--world`): what the world needs from the app each frame,
//! and back.

#[allow(unused_imports)]
use super::*;

/// Tell the world code about your character and the quality to draw at.
pub(super) fn world_in(
    w: Res<world::WorldClient>,
    av: Res<Avatar>,
    studio: Res<lighting::Studio>,
    mut me: ResMut<vox_world::LocalAvatar>,
    mut render: ResMut<vox_world::WorldRender>,
) {
    if !w.active() {
        return;
    }
    render.level = studio.level;
    render.shadows = studio.shadows;
    me.model = av.models[av.model].1.clone();
    me.fallback = av.models[0].1.clone();
    me.root = av.root;
    me.gltf = av.gltf.clone();
    me.ready = av.player.is_some();
    me.can_walk = av.clips.contains_key("Walk");
}

/// What the world decided: walk or stand, and that it lights the scene.
pub(super) fn world_out(
    w: Res<world::WorldClient>,
    me: Res<vox_world::LocalAvatar>,
    render: Res<vox_world::WorldRender>,
    mut av: ResMut<Avatar>,
    mut studio: ResMut<lighting::Studio>,
    mut walking: Local<bool>,
) {
    if !w.active() {
        return;
    }
    if me.walking != *walking {
        *walking = me.walking;
        av.base_loop = me.walking.then(|| "Walk".to_string());
    }
    if render.outdoors && !studio.outdoors {
        studio.outdoors = true;
    }
}
