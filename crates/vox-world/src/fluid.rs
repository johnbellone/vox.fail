//! Fluid in a jar (the Brain's head): a see-through layer that sloshes.
//!
//! A model marks its fluid with the glTF material `VoxelFluid` (tools/assets/build_animals.py
//! builds it as its own primitive). Each fluid mesh, as it spawns, gets its own
//! [`FluidMaterial`], drawn by `shaders/fluid.wesl`: everything above the surface is cut away,
//! so the jar shows a waterline, and the surface is worked out in **world** space, so it stays
//! level however the head tips. Every frame the surface is moved with the jar, and tilted by a
//! damped spring that the jar's acceleration pushes on (the fluid lags a nod or a hop, then rocks
//! back and forth and settles); small waves and rising bubbles run all the time.
//!
//! Shared by the avatar app and the world client; the avatar app's bevel pass leaves fluid alone
//! ([`crate::NoBevel`]), and [`adopt`] runs before it.

use bevy::{
    camera::primitives::MeshAabb,
    gltf::GltfMaterialName,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
    transform::TransformSystems,
};

/// The glTF material name that marks a fluid.
pub const MATERIAL_NAME: &str = "VoxelFluid";

/// How far below the fluid mesh's top the calm surface sits (m, in the model's own units), so the
/// waves and the tilt have room before they reach the mesh's flat top.
const HEADROOM: f32 = 0.085;
/// The sloshing's own frequency (Hz) and damping ratio: a few visible swings after a jolt.
const SLOSH_HZ: f32 = 1.9;
const SLOSH_DAMPING: f32 = 0.11;
/// The steepest the surface may lean (rise over run).
const MAX_TILT: f32 = 0.3;
/// The ripples that run all the time (m, before the model's scale).
const RIPPLE: f32 = 0.0025;

pub type FluidMaterial = ExtendedMaterial<StandardMaterial, FluidExt>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct FluidExt {
    /// the calm surface's centre in world space (xyz), the ripples' height (w, m)
    #[uniform(100)]
    pub surface: Vec4,
    /// the surface's slope in world space (x: rise per metre of x, y: per metre of z), the jar's
    /// scale (z), the waves' clock (w, s; it stands still while the app's time does)
    #[uniform(101)]
    pub slosh: Vec4,
}

impl MaterialExtension for FluidExt {
    fn fragment_shader() -> ShaderRef {
        "shaders/fluid.wesl".into()
    }
}

/// One jar's fluid: its material and the spring that tilts it.
#[derive(Component)]
pub struct Fluid {
    mat: Handle<FluidMaterial>,
    /// the calm surface's centre in the mesh's own space
    centre: Vec3,
    last: Option<Vec3>,
    vel: Vec3,
    acc: Vec3,
    tilt: Vec2,
    tilt_v: Vec2,
    clock: f32,
}

pub struct FluidPlugin;

impl Plugin for FluidPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<FluidMaterial>::default())
            .add_systems(PostUpdate, (adopt, slosh.after(TransformSystems::Propagate)));
    }
}

/// Gives every freshly spawned fluid mesh its own sloshing material.
#[allow(clippy::type_complexity)]
pub fn adopt(
    fresh: Query<(Entity, &GltfMaterialName, &Mesh3d, &MeshMaterial3d<StandardMaterial>), Without<Fluid>>,
    meshes: Res<Assets<Mesh>>,
    standard: Res<Assets<StandardMaterial>>,
    mut fluids: ResMut<Assets<FluidMaterial>>,
    mut commands: Commands,
) {
    for (e, name, mesh, mat) in &fresh {
        if name.0 != MATERIAL_NAME {
            continue;
        }
        let (Some(m), Some(base)) = (meshes.get(&mesh.0), standard.get(&mat.0)) else {
            continue;
        };
        let Some(bb) = m.get_aabb() else {
            continue;
        };
        let (lo, hi) = (Vec3::from(bb.min()), Vec3::from(bb.max()));
        let centre = Vec3::new((lo.x + hi.x) * 0.5, hi.y - HEADROOM, (lo.z + hi.z) * 0.5);
        let mut base = base.clone();
        base.alpha_mode = AlphaMode::Blend;
        base.double_sided = true;
        base.cull_mode = None;
        base.perceptual_roughness = 0.08;
        base.reflectance = 0.6;
        let h = fluids.add(FluidMaterial {
            base,
            extension: FluidExt {
                surface: Vec4::new(0.0, -1.0e4, 0.0, RIPPLE),
                slosh: Vec4::new(0.0, 0.0, 1.0, 0.0),
            },
        });
        commands
            .entity(e)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert((
                MeshMaterial3d(h.clone()),
                crate::NoBevel,
                Fluid {
                    mat: h,
                    centre,
                    last: None,
                    vel: Vec3::ZERO,
                    acc: Vec3::ZERO,
                    tilt: Vec2::ZERO,
                    tilt_v: Vec2::ZERO,
                    clock: 0.0,
                },
            ));
    }
}

/// Moves each surface with its jar and runs the slosh spring.
pub fn slosh(time: Res<Time>, mut jars: Query<(&mut Fluid, &GlobalTransform)>, mut mats: ResMut<Assets<FluidMaterial>>) {
    let dt = time.delta_secs();
    for (mut f, gt) in &mut jars {
        let at = gt.transform_point(f.centre);
        let scale = gt.compute_transform().scale.y.abs().max(1e-3);
        if dt > 1e-4 {
            if let Some(last) = f.last {
                let vel = (at - last) / dt;
                // smoothed: frame times jitter, and a raw second difference is mostly noise
                let acc = (vel - f.vel) / dt;
                f.acc = f.acc.lerp(acc, (dt * 20.0).min(1.0));
                f.vel = vel;
            }
            f.last = Some(at);
            // the surface leans to stay square to gravity less the jar's acceleration (a jar
            // pushed forward leaves its fluid behind), through a lightly damped spring
            let target = Vec2::new(-f.acc.x, -f.acc.z) / 9.8;
            let w = std::f32::consts::TAU * SLOSH_HZ;
            let pull = (target - f.tilt) * (w * w) - f.tilt_v * (2.0 * SLOSH_DAMPING * w);
            f.tilt_v += pull * dt;
            let tilt = f.tilt + f.tilt_v * dt;
            f.tilt = tilt.clamp(Vec2::splat(-MAX_TILT), Vec2::splat(MAX_TILT));
            f.clock += dt;
        } else if f.last.is_none() {
            f.last = Some(at);
        }
        if let Some(mut m) = mats.get_mut(&f.mat) {
            m.extension.surface = at.extend(RIPPLE * scale);
            m.extension.slosh = Vec4::new(f.tilt.x, f.tilt.y, scale, f.clock);
        }
    }
}
