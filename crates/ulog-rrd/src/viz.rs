//! The visualizations: each reads the tables it needs and writes its own entities.

use re_sdk_types::archetypes::{
    Arrows3D, LineStrips3D, Points3D, Scalars, SeriesLines, TextLog, Transform3D, ViewCoordinates,
};
use re_sdk_types::components::{Color, Radius, TextLogLevel};
use re_sdk_types::datatypes::Quaternion;

use crate::parse::{Table, Ulog};
use crate::write::Writer;
use crate::{err, Error};

/// The body axes are 0.3 m long; a 10 m/s velocity arrow draws as long as they are.
const AXIS_M: f32 = 0.3;
const VELOCITY_SCALE: f64 = AXIS_M as f64 / 10.0;
/// A topic sometimes carries a stray timestamp, such as ~2^32 µs, which would stretch the time axis
/// over hours of nothing. Samples this far outside the log's span are dropped.
const SLACK_US: u64 = 60_000_000;

/// PX4's north, east, down → Rerun's default z-up frame: east, north, up.
fn ned_to_enu(x: f64, y: f64, z: f64) -> [f32; 3] {
    [y as f32, x as f32, -z as f32]
}

/// The product of two rotations, each as (w, x, y, z): `a` after `b`.
fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [w1, x1, y1, z1] = a;
    let [w2, x2, y2, z2] = b;
    [
        w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2,
        w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2,
        w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2,
        w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2,
    ]
}

/// PX4's attitude, the rotation from the body frame (FRD) to the world (NED), as the rotation from the
/// body frame `vehicle/body` draws in, forward-left-up, to the z-up world, east-north-up: the NED→ENU
/// axis swap on the outside and the FRD→FLU one on the inside, each a half turn, so with the nose north
/// the body's x points north.
fn ned_quat_to_enu(w: f64, x: f64, y: f64, z: f64) -> Quaternion {
    const NED_TO_ENU: [f64; 4] = [0.0, std::f64::consts::FRAC_1_SQRT_2, std::f64::consts::FRAC_1_SQRT_2, 0.0];
    const FRD_TO_FLU: [f64; 4] = [0.0, 1.0, 0.0, 0.0];
    let [w, x, y, z] = qmul(qmul(NED_TO_ENU, [w, x, y, z]), FRD_TO_FLU);
    Quaternion::from_xyzw([x as f32, y as f32, z as f32, w as f32])
}

fn level(severity: u8) -> TextLogLevel {
    match severity {
        0 => TextLogLevel::CRITICAL,
        1..=3 => TextLogLevel::ERROR,
        4 => TextLogLevel::WARN,
        5 | 6 => TextLogLevel::INFO,
        _ => TextLogLevel::DEBUG,
    }
    .into()
}

/// The name of a MAV_CMD a mission item carries.
fn nav_cmd_label(nav_cmd: f64) -> &'static str {
    match nav_cmd as u16 {
        16 => "WP",
        17 => "LOITER",
        19 => "LOITER_T",
        20 => "RTL",
        21 => "LAND",
        22 => "TAKEOFF",
        84 => "VTOL_TAKEOFF",
        85 => "VTOL_LAND",
        _ => "CMD",
    }
}

/// The rows of `table` to draw: those within the log's span whose `fields` are all finite.
fn rows(log: &Ulog, table: &Table, fields: &[&[f64]]) -> Vec<usize> {
    let keep = log.start_us.saturating_sub(SLACK_US)..=log.end_us.saturating_add(SLACK_US);
    (0..table.timestamps.len())
        .filter(|&i| keep.contains(&table.timestamps[i]) && fields.iter().all(|f| f[i].is_finite()))
        .collect()
}

fn pick<T: Copy>(values: &[T], rows: &[usize]) -> Vec<T> {
    rows.iter().map(|&i| values[i]).collect()
}

pub fn visualize(log: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    match w.frame() {
        // Standalone, this frame is the world.
        None => w.static_row("", &ViewCoordinates::RFU())?,
        // Under a prefix the caller places it.
        Some((translation, rotation)) => {
            w.static_row("", &Transform3D::from_translation_rotation(translation, Quaternion::from_xyzw(rotation)))?
        }
    }
    pose(log, w)?;
    magnetometers(log, w)?;
    home(log, w)?;
    setpoint(log, w)?;
    mission(log, w)?;
    text(log, w)?;
    if w.scalars() {
        scalars(log, w)?;
    }
    Ok(())
}

/// The vehicle's estimated pose, its body axes, its velocity and the flown trail.
///
/// `vehicle` carries the position alone and `vehicle/body` the attitude, so a view with `vehicle` as
/// its origin follows the vehicle with the world's axes: a chase view. The trail sits in the world.
fn pose(log: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    let (Some(pos), Some(att)) = (log.table("vehicle_local_position"), log.table("vehicle_attitude")) else {
        return Ok(());
    };
    let (Some(x), Some(y), Some(z)) = (pos.get("x"), pos.get("y"), pos.get("z")) else { return Ok(()) };
    let q: Vec<&[f64]> = (0..4).filter_map(|i| att.get(&format!("q[{i}]"))).collect();
    if q.len() != 4 || att.timestamps.is_empty() {
        return Ok(());
    }
    let rows = rows(log, pos, &[x, y, z]);
    if rows.is_empty() {
        return Ok(());
    }
    let times = pick(&pos.timestamps, &rows);

    w.static_row(
        "vehicle/body/frd",
        &Arrows3D::from_vectors([[AXIS_M, 0.0, 0.0], [0.0, -AXIS_M, 0.0], [0.0, 0.0, -AXIS_M]]).with_colors([
            Color::from_rgb(255, 0, 0),
            Color::from_rgb(0, 255, 0),
            Color::from_rgb(0, 0, 255),
        ]),
    )?;
    w.static_row(
        "vehicle/body/marker",
        &Points3D::new([[0.0, 0.0, 0.0]])
            .with_radii([Radius::new_ui_points(5.0)])
            .with_colors([Color::from_rgb(255, 255, 255)]),
    )?;

    let positions: Vec<[f32; 3]> = rows.iter().map(|&i| ned_to_enu(x[i], y[i], z[i])).collect();
    w.columns(
        "vehicle",
        &times,
        Transform3D::update_fields()
            .with_many_translation(positions.iter().copied())
            .columns_of_unit_batches()
            .map_err(err)?,
    )?;

    // The attitude sample nearest in time to each position sample.
    let mut j = 0;
    let rotations: Vec<Quaternion> = rows
        .iter()
        .map(|&i| {
            let ts = pos.timestamps[i];
            while j + 1 < att.timestamps.len() && att.timestamps[j + 1].abs_diff(ts) <= att.timestamps[j].abs_diff(ts) {
                j += 1;
            }
            ned_quat_to_enu(q[0][j], q[1][j], q[2][j], q[3][j])
        })
        .collect();
    w.columns(
        "vehicle/body",
        &times,
        Transform3D::update_fields().with_many_quaternion(rotations).columns_of_unit_batches().map_err(err)?,
    )?;

    if let (Some(vx), Some(vy), Some(vz)) = (pos.get("vx"), pos.get("vy"), pos.get("vz")) {
        let vectors =
            rows.iter().map(|&i| ned_to_enu(vx[i] * VELOCITY_SCALE, vy[i] * VELOCITY_SCALE, vz[i] * VELOCITY_SCALE));
        let labels = rows.iter().map(|&i| format!("{:.1} m/s", (vx[i] * vx[i] + vy[i] * vy[i] + vz[i] * vz[i]).sqrt()));
        w.static_row(
            "vehicle/velocity",
            &Arrows3D::update_fields().with_colors([Color::from_rgb(0, 255, 255)]).with_show_labels(true),
        )?;
        w.columns(
            "vehicle/velocity",
            &times,
            Arrows3D::update_fields()
                .with_vectors(vectors)
                .with_origins(positions.iter().copied())
                .with_labels(labels)
                .columns_of_unit_batches()
                .map_err(err)?,
        )?;
    }

    w.static_row(
        "trail",
        &LineStrips3D::new([positions]).with_colors([Color::from_unmultiplied_rgba(255, 255, 255, 200)]),
    )
}

fn mag_color(index: usize) -> Color {
    [
        Color::from_rgb(239, 85, 59),
        Color::from_rgb(0, 204, 150),
        Color::from_rgb(99, 110, 250),
        Color::from_rgb(254, 203, 82),
    ][index % 4]
}

/// Each magnetometer's field in the body frame, and the samples its calibration collected.
fn magnetometers(log: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    for n in 0..4 {
        let topic = if n == 0 { "sensor_mag".to_owned() } else { format!("sensor_mag_{n}") };
        if let Some(mag) = log.table(&topic) {
            if let (Some(x), Some(y), Some(z)) = (mag.get("x"), mag.get("y"), mag.get("z")) {
                let rows = rows(log, mag, &[x, y, z]);
                if !rows.is_empty() {
                    let path = format!("vehicle/body/sensor_mag/{n}");
                    w.static_row(&path, &Arrows3D::update_fields().with_colors([mag_color(n)]))?;
                    w.columns(
                        &path,
                        &pick(&mag.timestamps, &rows),
                        Arrows3D::update_fields()
                            .with_vectors(rows.iter().map(|&i| [x[i] as f32, y[i] as f32, z[i] as f32]))
                            .columns_of_unit_batches()
                            .map_err(err)?,
                    )?;
                }
            }
        }
    }
    // The calibration worker reports one sample per magnetometer per message, as arrays.
    let Some(cal) = log.table("mag_worker_data") else { return Ok(()) };
    for n in 0..4 {
        let (Some(x), Some(y), Some(z)) =
            (cal.get(&format!("x[{n}]")), cal.get(&format!("y[{n}]")), cal.get(&format!("z[{n}]")))
        else {
            break;
        };
        let rows = rows(log, cal, &[x, y, z]);
        let samples: Vec<[f32; 3]> = rows
            .iter()
            .map(|&i| [x[i] as f32, y[i] as f32, z[i] as f32])
            .filter(|p| p.iter().any(|v| *v != 0.0))
            .collect();
        if !samples.is_empty() {
            w.static_row(
                &format!("vehicle/body/mag_cal/{n}"),
                &Points3D::new(samples).with_colors([mag_color(n)]).with_radii([Radius::new_scene_units(0.01)]),
            )?;
        }
    }
    Ok(())
}

/// A point sized in screen points, so it stays visible over a flight of any size.
fn marker(color: Color, ui_radius: f32, show_labels: bool) -> Points3D {
    Points3D::update_fields()
        .with_colors([color])
        .with_radii([Radius::new_ui_points(ui_radius)])
        .with_show_labels(show_labels)
}

/// The home position, each time PX4 sets it.
fn home(log: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    let Some(home) = log.table("home_position") else { return Ok(()) };
    let (Some(x), Some(y), Some(z)) = (home.get("x"), home.get("y"), home.get("z")) else { return Ok(()) };
    let rows = rows(log, home, &[x, y, z]);
    if rows.is_empty() {
        return Ok(());
    }
    w.static_row("home", &marker(Color::from_rgb(0, 255, 0), 5.0, true).with_labels(["Home"]))?;
    w.columns(
        "home",
        &pick(&home.timestamps, &rows),
        Points3D::update_fields()
            .with_positions(rows.iter().map(|&i| ned_to_enu(x[i], y[i], z[i])))
            .columns_of_unit_batches()
            .map_err(err)?,
    )
}

/// The position setpoint and its yaw.
fn setpoint(log: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    let Some(sp) = log.table("vehicle_local_position_setpoint") else { return Ok(()) };
    let (Some(x), Some(y), Some(z)) = (sp.get("x"), sp.get("y"), sp.get("z")) else { return Ok(()) };
    let rows = rows(log, sp, &[x, y, z]);
    if rows.is_empty() {
        return Ok(());
    }
    let positions: Vec<[f32; 3]> = rows.iter().map(|&i| ned_to_enu(x[i], y[i], z[i])).collect();
    w.static_row("setpoint", &marker(Color::from_unmultiplied_rgba(255, 255, 255, 120), 3.0, false))?;
    w.columns(
        "setpoint",
        &pick(&sp.timestamps, &rows),
        Points3D::update_fields().with_positions(positions.iter().copied()).columns_of_unit_batches().map_err(err)?,
    )?;
    let Some(yaw) = sp.get("yaw") else { return Ok(()) };
    let rows: Vec<usize> = rows.into_iter().filter(|&i| yaw[i].is_finite()).collect();
    if rows.is_empty() {
        return Ok(());
    }
    // Yaw is clockwise from north in NED; the arrow is as long as the body axes.
    let fwd = rows.iter().map(|&i| ned_to_enu(yaw[i].cos() * AXIS_M as f64, yaw[i].sin() * AXIS_M as f64, 0.0));
    w.static_row("setpoint/yaw", &Arrows3D::update_fields().with_colors([Color::from_rgb(255, 0, 0)]))?;
    w.columns(
        "setpoint/yaw",
        &pick(&sp.timestamps, &rows),
        Arrows3D::update_fields()
            .with_vectors(fwd)
            .with_origins(rows.iter().map(|&i| ned_to_enu(x[i], y[i], z[i])))
            .columns_of_unit_batches()
            .map_err(err)?,
    )
}

/// The current mission item, from the moment it becomes current, in the local frame.
fn mission(log: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    const EARTH_RADIUS_M: f64 = 6_371_000.0;
    let (Some(items), Some(pos)) = (log.table("navigator_mission_item"), log.table("vehicle_local_position")) else {
        return Ok(());
    };
    // The local frame's reference: the first sample that has one.
    let (Some(xy_global), Some(ref_lat), Some(ref_lon), Some(ref_alt)) =
        (pos.get("xy_global"), pos.get("ref_lat"), pos.get("ref_lon"), pos.get("ref_alt"))
    else {
        return Ok(());
    };
    let Some(r) = xy_global.iter().position(|v| *v != 0.0) else { return Ok(()) };
    let (lat0, lon0, alt0) = (ref_lat[r], ref_lon[r], ref_alt[r]);
    let (Some(lat), Some(lon), Some(alt), Some(cmd)) =
        (items.get("latitude"), items.get("longitude"), items.get("altitude"), items.get("nav_cmd"))
    else {
        return Ok(());
    };
    let relative = items.get("altitude_is_relative");
    // An equirectangular projection about the reference.
    let local = |i: usize| {
        let north = (lat[i] - lat0).to_radians() * EARTH_RADIUS_M;
        let east = (lon[i] - lon0).to_radians() * EARTH_RADIUS_M * lat0.to_radians().cos();
        let down = if relative.is_some_and(|r| r[i] != 0.0) { -alt[i] } else { -(alt[i] - alt0) };
        [north, east, down]
    };
    let rows: Vec<usize> =
        rows(log, items, &[lat, lon, alt]).into_iter().filter(|&i| local(i).iter().all(|v| v.is_finite())).collect();
    if rows.is_empty() {
        return Ok(());
    }
    w.static_row("mission_item", &marker(Color::from_rgb(255, 165, 0), 6.0, true))?;
    w.columns(
        "mission_item",
        &pick(&items.timestamps, &rows),
        Points3D::update_fields()
            .with_positions(rows.iter().map(|&i| {
                let [n, e, d] = local(i);
                ned_to_enu(n, e, d)
            }))
            .with_labels(rows.iter().map(|&i| nav_cmd_label(cmd[i])))
            .columns_of_unit_batches()
            .map_err(err)?,
    )
}

/// PX4's text messages.
fn text(log: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    if log.messages.is_empty() {
        return Ok(());
    }
    let times: Vec<u64> = log.messages.iter().map(|m| m.timestamp).collect();
    w.columns(
        "log",
        &times,
        TextLog::update_fields()
            .with_many_text(log.messages.iter().map(|m| m.text.as_str()))
            .with_many_level(log.messages.iter().map(|m| level(m.severity)))
            .columns_of_unit_batches()
            .map_err(err)?,
    )
}

/// Every numeric field of every topic: one entity per topic instance with a named series per field.
/// One entity per topic rather than per field, since each row carries a row id and a timestamp,
/// which per-field entities would repeat for every field of every message.
fn scalars(log: &Ulog, w: &mut Writer<'_>) -> Result<(), Error> {
    let mut topics: Vec<(&String, &Table)> = log.tables.iter().collect();
    topics.sort_by(|a, b| a.0.cmp(b.0));
    for (topic, table) in topics {
        let rows = rows(log, table, &[]);
        if rows.is_empty() || table.columns.is_empty() {
            continue;
        }
        let path = format!("timeseries/{topic}");
        w.static_row(&path, &SeriesLines::new().with_names(table.columns.iter().map(|(name, _)| name.as_str())))?;
        let width = table.columns.len();
        let values = rows.iter().flat_map(|&i| table.columns.iter().map(move |(_, column)| column[i]));
        w.columns(
            &path,
            &pick(&table.timestamps, &rows),
            Scalars::new(values).columns(std::iter::repeat_n(width, rows.len())).map_err(err)?,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `v` rotated by the quaternion, (x, y, z, w) as Rerun keeps it.
    fn rotate(q: Quaternion, v: [f64; 3]) -> [f64; 3] {
        let [x, y, z, w] = q.0.map(f64::from);
        let r = qmul(qmul([w, x, y, z], [0.0, v[0], v[1], v[2]]), [w, -x, -y, -z]);
        [r[1], r[2], r[3]]
    }

    fn close(a: [f64; 3], b: [f64; 3]) -> bool {
        a.iter().zip(b).all(|(p, q)| (p - q).abs() < 1e-6)
    }

    #[test]
    fn the_body_axes_land_where_px4_points_them() {
        // Nose north, level: body forward is north, body right is east, body down is down.
        let level = ned_quat_to_enu(1.0, 0.0, 0.0, 0.0);
        assert!(close(rotate(level, [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]), "forward");
        assert!(close(rotate(level, [0.0, -1.0, 0.0]), [1.0, 0.0, 0.0]), "right, drawn as -y");
        assert!(close(rotate(level, [0.0, 0.0, -1.0]), [0.0, 0.0, -1.0]), "down, drawn as -z");
        // A quarter turn right about down, PX4's positive yaw: the nose points east.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let east = ned_quat_to_enu(s, 0.0, 0.0, s);
        assert!(close(rotate(east, [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]), "yawed forward");
    }
}
