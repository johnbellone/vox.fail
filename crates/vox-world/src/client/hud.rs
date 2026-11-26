//! The weather between keyframes, and the HUD text and crosshair.

use super::*;

/// Run the weather between the server's keyframes (four times a second).
pub fn weather(mut w: ResMut<WorldClient>, real: Res<Time<Real>>) {
    if w.weather.is_none() {
        return;
    }
    w.weather_acc += real.delta_secs();
    if w.weather_acc >= 0.25 {
        let dt = w.weather_acc.min(2.0);
        w.weather_acc = 0.0;
        if let Some(wx) = &mut w.weather {
            wx.step(dt);
        }
    }
}

/// "from the south-west" for a wind blowing towards (x, z).
pub(super) fn compass(wind: [f32; 3]) -> &'static str {
    // where it comes from: opposite to where it blows, clockwise from north (-z)
    let from = (-wind[0]).atan2(wind[2]).to_degrees().rem_euclid(360.0);
    const NAMES: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    NAMES[((from + 22.5) / 45.0) as usize % 8]
}

/// The weather where you are, in a line.
pub fn weather_line(a: &Air) -> String {
    let speed = a.wind[0].hypot(a.wind[2]);
    let vis = if a.visibility >= 10_000.0 { format!("{:.0} km", a.visibility / 1000.0) } else { format!("{:.0} m", a.visibility) };
    let mut s = format!(
        "{:.0} C   humidity {:.0}%   dew point {:.0} C   wind {:.1} m/s from {}   visibility {}",
        a.temperature,
        a.humidity * 100.0,
        a.dew_point,
        speed,
        compass(a.wind),
        vis
    );
    if a.rain > 0.2 {
        s += &format!("   {} {:.1} mm/h", if a.temperature < 0.5 { "snow" } else { "rain" }, a.rain);
    }
    s
}

/// A crosshair, the block bar and what's happening.
pub fn hud(
    mut w: ResMut<WorldClient>,
    mut commands: Commands,
    mut texts: Query<&mut Text, With<WorldHud>>,
    mut crosshairs: Query<&mut Visibility, With<Crosshair>>,
    pixel: Option<Res<crate::hud::PixelHud>>,
) {
    if !w.active() || w.watch {
        return;
    }
    // with the pixel HUD (the hotbar and hints drawn as such), this text is just the status,
    // small, top left
    let pixel = pixel.is_some();
    let hud = match w.hud {
        Some(e) => e,
        None => {
            commands.spawn((
                Text::new("+"),
                TextFont { font_size: bevy::text::FontSize::Px(28.0), ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                TextShadow::default(),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(50.0),
                    top: Val::Percent(50.0),
                    margin: UiRect { left: Val::Px(-8.0), top: Val::Px(-18.0), ..default() },
                    ..default()
                },
                Crosshair,
            ));
            // on a dark glass panel, so it reads on the bright void as well as at night
            let node = if pixel {
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(12.0),
                    left: Val::Px(12.0),
                    max_width: Val::Percent(60.0),
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                }
            } else {
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(12.0),
                    left: Val::Px(12.0),
                    right: Val::Px(12.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    justify_content: JustifyContent::Center,
                    ..default()
                }
            };
            let e = commands
                .spawn((
                    Text::new(""),
                    TextFont { font_size: bevy::text::FontSize::Px(if pixel { 13.0 } else { 16.0 }), ..default() },
                    TextColor(Color::srgb(0.96, 0.96, 0.98)),
                    TextLayout::justify(if pixel { Justify::Left } else { Justify::Center }),
                    node,
                    BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.55)),
                    WorldHud,
                ))
                .id();
            w.hud = Some(e);
            e
        }
    };
    let now = w.now();
    if w.notice.as_ref().is_some_and(|(_, at)| now - at > 3.0) {
        w.notice = None;
    }
    let bar: Vec<String> = w
        .hotbar
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let n = w.block_name(*b);
            if i == w.slot { format!("[{} {n}]", i + 1) } else { format!("{} {n}", i + 1) }
        })
        .collect();
    let mut line = bar.join("   ");
    let who = w.players();
    let top = match &w.notice {
        Some((n, _)) => n.clone(),
        None if !w.ready => w.status.clone(),
        None if who.is_empty() => format!("{} | on your own{}", w.world, if w.fly { " | flying" } else { "" }),
        None => format!("{} | with {}{}", w.world, who.join(", "), if w.fly { " | flying" } else { "" }),
    };
    line = if pixel { top } else { format!("{top}\n{line}") };
    if let Some(a) = w.air() {
        line = format!("{}\n{line}", weather_line(&a));
    }
    // the crosshair when looking with the mouse: from above the mouse is the pointer
    let show = if w.cam_mode != CamMode::Diablo && w.shot.is_none() { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut crosshairs {
        if *v != show {
            *v = show;
        }
    }
    if let Ok(mut t) = texts.get_mut(hud) {
        if t.0 != line {
            t.0 = line;
        }
    }
}
