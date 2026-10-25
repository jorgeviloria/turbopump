//! Back vanes ("álabes de rebombeo"): short radial ribs on the back face of the hub
//! disk that drag the fluid of the rear chamber round with the impeller. The rotation
//! of the fluid rises, the pressure on the back of the disk falls and so does the axial
//! thrust.
//!
//! Defaults follow the tests tabulated by Gülich (Table 9.3, after Möhring's thesis,
//! book p. 534; §9.2.7, p. 533), with r₂ = d₂/2:
//! - number: 4 if d₂ < 200 mm, else 6 (the source gives 6 to 8 above 200 mm);
//! - outer diameter: 0.80·d₂ (source range 0.75 to 0.85);
//! - height h: 0.04·r₂ (source range 0.03 to 0.05);
//! - tip clearance: 0.2·h = 0.008·r₂ (source ranges s/h = 0.1 to 0.2 and
//!   s/r₂ = 0.008 to 0.015);
//! - vane width, the circumferential thickness: 2·h;
//! - shape: radial and straight (§9.2.7); a curved vane follows the main blades.
//!
//! The inner diameter has no value in the source (its figure draws the vanes down to
//! the hub), so it is our choice: the boss radius plus the wall thickness. With the
//! rear seal on, the vanes stop outside the casing ring of the seal (also our choice,
//! with a warning: Gülich's impellers with radial vanes have no rear ring).

use serde::{Deserialize, Serialize};
use tp_math::DVec2;

use super::ImpellerBasis;
use super::material::Material;
use super::seals::Seal;
use super::wall::{Wall, offset_points};
use crate::Param;

/// Points of the disk wire used to find its back face.
const FACE_SAMPLES: usize = 600;

/// Outlet diameter below which 4 vanes are used, m (Gülich Table 9.3).
const SMALL_PUMP_DIAMETER: f64 = 0.2;
/// d_Rs/d₂ (Gülich Table 9.3, 0.75 to 0.85).
pub const OUTER_DIAMETER_RATIO: f64 = 0.80;
/// h/r₂ (Gülich Table 9.3, 0.03 to 0.05).
pub const HEIGHT_RATIO: f64 = 0.04;
/// s_v/h (Gülich Table 9.3, 0.1 to 0.2).
pub const TIP_CLEARANCE_RATIO: f64 = 0.2;
/// Vane width over its height (Gülich Table 9.3).
pub const THICKNESS_RATIO: f64 = 2.0;

/// How the vanes run in plan view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum VaneShape {
    /// Straight and radial (Gülich §9.2.7).
    #[default]
    Radial,
    /// A logarithmic spiral θ(r) = ln(r/r_in)/tan(angle) turned in the sense of
    /// the main blades; `angle`, rad, is measured from the circumferential
    /// direction, like the blade angle β.
    Curved { angle: f64 },
}

/// Inputs of the back vanes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackVaneSettings {
    /// Whether the impeller has back vanes; off by default.
    pub enabled: bool,
    /// Number z_v; automatic: 4 below d₂ = 200 mm, else 6.
    pub count: Param<u32>,
    pub shape: VaneShape,
    /// Outer diameter d_Rs, m; automatic: 0.80·d₂.
    pub outer_diameter: Param<f64>,
    /// Inner diameter, m; automatic: the boss diameter plus two wall thicknesses (our
    /// choice).
    pub inner_diameter: Param<f64>,
    /// Axial height h, m; automatic: 0.04·r₂.
    pub height: Param<f64>,
    /// Circumferential width of a vane, m; automatic: 2·h.
    pub thickness: Param<f64>,
    /// Axial clearance s_v between the vane tips and the casing, m; automatic: 0.2·h.
    pub tip_clearance: Param<f64>,
}

impl Default for BackVaneSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            count: Param::Auto,
            shape: VaneShape::Radial,
            outer_diameter: Param::Auto,
            inner_diameter: Param::Auto,
            height: Param::Auto,
            thickness: Param::Auto,
            tip_clearance: Param::Auto,
        }
    }
}

/// The back vanes as used.
#[derive(Debug, Clone, PartialEq)]
pub struct BackVanes {
    pub count: u32,
    pub shape: VaneShape,
    /// Circumferential width of a vane, m.
    pub thickness: f64,
    pub tip_clearance: f64,
    /// Closed meridional band (z, r), m, without repeating its first point: the back
    /// face from the outer to the inner radius, then the same face moved `height` back,
    /// from the inner to the outer radius. Empty when the vanes have no room.
    pub band: Vec<DVec2>,
    pub inner_radius: f64,
    pub outer_radius: f64,
    pub height: f64,
    pub warnings: Vec<String>,
}

impl BackVanes {
    /// The largest axial position z of the vane tips, m; `None` without a band.
    pub fn tip_z(&self) -> Option<f64> {
        self.band.iter().map(|p| p.x).reduce(f64::max)
    }
}

/// The part of the polyline `face`, running from large to small radius, between the
/// radii `outer` and `inner`, its ends interpolated onto them.
fn clip(face: &[DVec2], outer: f64, inner: f64) -> Vec<DVec2> {
    let at = |a: DVec2, b: DVec2, y: f64| {
        let f = if (a.y - b.y).abs() > 0.0 {
            (a.y - y) / (a.y - b.y)
        } else {
            0.0
        };
        a.lerp(b, f.clamp(0.0, 1.0))
    };
    let mut out: Vec<DVec2> = Vec::new();
    for w in face.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b.y > outer {
            continue;
        }
        if out.is_empty() {
            out.push(if a.y > outer { at(a, b, outer) } else { a });
        }
        if b.y <= inner {
            out.push(at(a, b, inner));
            return out;
        }
        out.push(b);
    }
    out
}

/// Builds the back vanes on the back of the hub `disk`, `None` when they are off.
/// `inside` is a point of the channel, to tell the back from the front of the disk.
pub fn build(
    disk: &Wall,
    inside: DVec2,
    settings: &BackVaneSettings,
    basis: &ImpellerBasis,
    material: &Material,
    rear: Option<&Seal>,
    eyes: f64,
) -> Option<BackVanes> {
    if !settings.enabled {
        return None;
    }
    let mut warnings = Vec::new();
    let r2 = basis.outlet_diameter / 2.0;
    let count = settings
        .count
        .resolve(|| {
            if basis.outlet_diameter < SMALL_PUMP_DIAMETER {
                4
            } else {
                6
            }
        })
        .max(1);
    let height = settings.height.resolve(|| HEIGHT_RATIO * r2).max(1e-6);
    let thickness = settings
        .thickness
        .resolve(|| THICKNESS_RATIO * height)
        .max(1e-6);
    let tip_clearance = settings
        .tip_clearance
        .resolve(|| TIP_CLEARANCE_RATIO * height)
        .max(0.0);

    if eyes > 1.5 {
        // A double-entry impeller has no rear shroud to carry them: none is built.
        warnings.push(
            "a double-entry impeller has no rear shroud: the back vanes are left out".to_string(),
        );
        return Some(BackVanes {
            count,
            shape: settings.shape,
            thickness,
            tip_clearance,
            band: Vec::new(),
            inner_radius: 0.0,
            outer_radius: 0.0,
            height,
            warnings,
        });
    }

    // The back face from the rim inwards, as the rear chamber takes it.
    let points = disk.sample(FACE_SAMPLES);
    let rim = disk.segments()[0].end();
    let nearest = points
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.distance(rim).total_cmp(&b.1.distance(rim)))
        .map_or(0, |(i, _)| i);
    let back = &points[nearest..];

    let mut outer = settings
        .outer_diameter
        .resolve(|| OUTER_DIAMETER_RATIO * basis.outlet_diameter)
        / 2.0;
    if outer > rim.y {
        warnings.push(format!(
            "the back vanes reach out to {:.1} mm, beyond the disk rim ({:.1} mm): they stop at the rim",
            outer * 1e3,
            rim.y * 1e3
        ));
        outer = rim.y;
    }
    let mut inner = settings
        .inner_diameter
        .resolve(|| material.boss_diameter + 2.0 * material.wall_thickness)
        / 2.0;
    if let Some(seal) = rear
        && inner < seal.casing_seat
    {
        warnings.push(format!(
            "the back vanes stop outside the rear casing ring: their inner radius is raised from {:.1} mm to {:.1} mm",
            inner * 1e3,
            seal.casing_seat * 1e3
        ));
        inner = seal.casing_seat;
    }
    if f64::from(count) * thickness > 2.0 * std::f64::consts::PI * inner {
        warnings.push(format!(
            "the {count} back vanes {:.1} mm wide overlap at their inner radius ({:.1} mm)",
            thickness * 1e3,
            inner * 1e3
        ));
    }
    let vanes = |band: Vec<DVec2>, warnings: Vec<String>| BackVanes {
        count,
        shape: settings.shape,
        thickness,
        tip_clearance,
        band,
        inner_radius: inner,
        outer_radius: outer,
        height,
        warnings,
    };
    if outer <= inner + 1e-6 {
        warnings.push(format!(
            "the back vanes have no room: the outer radius ({:.1} mm) is not above the inner one ({:.1} mm)",
            outer * 1e3,
            inner * 1e3
        ));
        return Some(vanes(Vec::new(), warnings));
    }
    let face = clip(back, outer, inner);
    if face.len() < 2 {
        warnings.push("the back vanes find no back face of the disk between their radii".into());
        return Some(vanes(Vec::new(), warnings));
    }
    let moved = offset_points(&face, height, inside).unwrap_or_else(|_| {
        warnings.push(format!(
            "the back vanes {:.1} mm high do not fit the curvature of the back face; the face is moved straight back",
            height * 1e3
        ));
        face.iter().map(|p| *p + DVec2::new(height, 0.0)).collect()
    });
    // The copy is cut at the same radii: a face leaning over its normal would otherwise
    // carry the tips past them.
    let moved = clip(&moved, outer, inner);
    let mut band = face;
    band.extend(moved.into_iter().rev());
    Some(vanes(band, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::meridian::{MeridionalContour, MeridionalInput, tests::basis};

    fn contour(input: &MeridionalInput) -> MeridionalContour {
        MeridionalContour::build(&basis(), input, 1.0).unwrap()
    }

    fn with_vanes(rear_seal: bool, vanes: BackVaneSettings) -> MeridionalInput {
        let mut input = MeridionalInput::default();
        input.seals.rear.enabled = rear_seal;
        input.back_vanes = BackVaneSettings {
            enabled: true,
            ..vanes
        };
        input
    }

    #[test]
    fn a_double_entry_impeller_gets_no_back_vanes() {
        let c = MeridionalContour::build(
            &basis(),
            &with_vanes(false, BackVaneSettings::default()),
            2.0,
        )
        .unwrap();
        // The vanes are not built (no band) and the contour says why.
        assert!(c.back_vanes.as_ref().is_none_or(|v| v.band.is_empty()));
        assert!(
            c.material_warnings
                .iter()
                .any(|w| w.contains("double-entry impeller has no rear shroud")),
            "{:?}",
            c.material_warnings
        );
    }

    #[test]
    fn back_vanes_are_off_by_default() {
        assert!(!BackVaneSettings::default().enabled);
        assert!(contour(&MeridionalInput::default()).back_vanes.is_none());
        // An old project has no key for them and loads with them off.
        let input: MeridionalInput = serde_json::from_str("{}").unwrap();
        assert!(!input.back_vanes.enabled);
    }

    #[test]
    fn the_automatic_values_follow_gulich_table_9_3() {
        let b = basis();
        let r2 = b.outlet_diameter / 2.0;
        let c = contour(&with_vanes(false, BackVaneSettings::default()));
        let v = c.back_vanes.expect("vanes");
        assert_eq!(v.count, 4);
        assert!((v.outer_radius - 0.8 * r2).abs() < 1e-12);
        assert!((v.height - 0.04 * r2).abs() < 1e-12);
        assert!((v.tip_clearance - 0.2 * v.height).abs() < 1e-15);
        assert!((v.thickness - 2.0 * v.height).abs() < 1e-15);
        let t = c.material.wall_thickness;
        assert!((v.inner_radius - (c.material.boss_diameter / 2.0 + t)).abs() < 1e-12);
        // 6 vanes from d₂ = 200 mm on.
        let big = ImpellerBasis {
            outlet_diameter: 0.25,
            ..b
        };
        let c =
            MeridionalContour::build(&big, &with_vanes(false, BackVaneSettings::default()), 1.0)
                .unwrap();
        assert_eq!(c.back_vanes.unwrap().count, 6);
    }

    #[test]
    fn the_band_lies_behind_the_disk_at_the_vane_height() {
        let c = contour(&with_vanes(false, BackVaneSettings::default()));
        let v = c.back_vanes.as_ref().expect("vanes");
        assert!(v.warnings.is_empty(), "{:?}", v.warnings);
        let face = c.hub_material.sample(4000);
        let distance = |p: DVec2| {
            face.iter()
                .map(|q| q.distance(p))
                .fold(f64::INFINITY, f64::min)
        };
        // Every point is on the face itself or on its copy moved back by the height.
        let (mut on_face, mut on_tips) = (0, 0);
        for p in &v.band {
            let d = distance(*p);
            if d < 0.01 * v.height {
                on_face += 1;
            } else if (d - v.height).abs() < 0.01 * v.height {
                on_tips += 1;
            } else {
                panic!("{p:?} is {d} from the face");
            }
            assert!(p.y <= v.outer_radius + 1e-9 && p.y >= v.inner_radius - 1e-9);
        }
        assert!(on_face > 10 && on_tips > 10, "{on_face} {on_tips}");
        // Behind the disk: the tips lie at larger z than the face at the same radius.
        assert!(v.tip_z().unwrap() > v.band[0].x);
    }

    #[test]
    fn vanes_stop_outside_the_rear_casing_ring_with_a_warning() {
        let c = contour(&with_vanes(
            true,
            BackVaneSettings {
                inner_diameter: Param::Manual(0.02),
                ..BackVaneSettings::default()
            },
        ));
        let rear = c.seals.rear.expect("rear seal");
        let v = c.back_vanes.expect("vanes");
        assert!(v.inner_radius >= rear.casing_seat);
        assert!(
            v.warnings.iter().any(|w| w.contains("rear casing ring")),
            "{:?}",
            v.warnings
        );
        assert!(
            c.material_warnings
                .iter()
                .any(|w| w.contains("rear casing ring"))
        );
    }

    #[test]
    fn the_rear_chamber_clears_the_vane_tips() {
        // Vanes taller than the axial gap of the chamber push the casing wall back.
        let r2 = basis().outlet_diameter / 2.0;
        let c = contour(&with_vanes(
            false,
            BackVaneSettings {
                height: Param::Manual(0.10 * r2),
                ..BackVaneSettings::default()
            },
        ));
        let v = c.back_vanes.as_ref().expect("vanes");
        let wall_z = c.chambers.rear_casing.start().x;
        // The wall clears the tips by the clearance; where the face leans, the tips are
        // a height from it along its normal, so the axial distance may be a bit shorter.
        let face_z = c
            .hub_material
            .sample(4000)
            .iter()
            .filter(|p| p.y >= v.inner_radius && p.y <= v.outer_radius)
            .map(|p| p.x)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            wall_z >= face_z + v.height + v.tip_clearance - 0.01 * v.height,
            "{wall_z} {face_z}"
        );
        assert!((wall_z - v.tip_z().unwrap() - v.tip_clearance).abs() < 1e-9);
        // At the default vane settings the plate sits at the same exact gap, not at s_ax.
        let d = contour(&with_vanes(false, BackVaneSettings::default()));
        let v = d.back_vanes.as_ref().expect("vanes");
        let wall = d.chambers.rear_casing.start().x;
        assert!((wall - v.tip_z().unwrap() - v.tip_clearance).abs() < 1e-12);
        // Without vanes the plain s_ax stays.
        let mut input = MeridionalInput::default();
        input.seals.rear.enabled = false;
        let plain = contour(&input);
        assert_ne!(wall, plain.chambers.rear_casing.start().x);
    }

    #[test]
    fn the_rear_chamber_gap_follows_the_tip_clearance() {
        let wall = |clearance: f64| {
            contour(&with_vanes(
                false,
                BackVaneSettings {
                    tip_clearance: Param::Manual(clearance),
                    ..BackVaneSettings::default()
                },
            ))
            .chambers
            .rear_casing
            .start()
            .x
        };
        assert!((wall(0.003) - wall(0.001) - 0.002).abs() < 1e-12);
    }

    #[test]
    fn the_collar_of_the_rear_seal_keeps_the_plate_back_with_a_warning() {
        let mut input = with_vanes(true, BackVaneSettings::default());
        input.chambers.enabled = true;
        let c = contour(&input);
        let v = c.back_vanes.as_ref().expect("vanes");
        let ch = &c.chambers;
        let wall = ch.rear_casing.start().x;
        let rear = c.seals.rear.expect("rear seal");
        let collar_tip = c
            .hub_material
            .sample(4000)
            .iter()
            .filter(|p| (p.y - rear.impeller_seat).abs() < 1e-9)
            .map(|p| p.x)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(wall >= collar_tip + ch.values.axial_gap - 1e-9);
        let gap = ch.values.rear_vane_gap.expect("gap");
        assert!((gap - (wall - v.tip_z().unwrap())).abs() < 1e-12);
        assert!(gap > v.tip_clearance);
        assert!(
            ch.warnings.iter().any(|w| w.contains("collar")),
            "{:?}",
            ch.warnings
        );
        // Without the rear seal the gap is the clearance, and without vanes there is none.
        let c = contour(&with_vanes(false, BackVaneSettings::default()));
        let v = c.back_vanes.as_ref().unwrap();
        assert!((c.chambers.values.rear_vane_gap.unwrap() - v.tip_clearance).abs() < 1e-12);
        assert!(
            contour(&MeridionalInput::default())
                .chambers
                .values
                .rear_vane_gap
                .is_none()
        );
    }

    #[test]
    fn six_vanes_from_exactly_200_mm_and_overlap_warns() {
        let b = ImpellerBasis {
            outlet_diameter: 0.200,
            ..basis()
        };
        let c = MeridionalContour::build(&b, &with_vanes(false, BackVaneSettings::default()), 1.0)
            .unwrap();
        assert_eq!(c.back_vanes.unwrap().count, 6);
        let crowded = contour(&with_vanes(
            false,
            BackVaneSettings {
                count: Param::Manual(40),
                ..BackVaneSettings::default()
            },
        ));
        assert!(
            crowded
                .back_vanes
                .unwrap()
                .warnings
                .iter()
                .any(|w| w.contains("overlap"))
        );
    }
}
