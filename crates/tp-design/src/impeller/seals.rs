//! The annular seals of the impeller in the meridional plane: the front seal at the eye
//! and the rear seal on the hub side, each with an optional pair of replaceable wear
//! rings (one pressed on the impeller, one in the casing).
//!
//! Radii, from the axis outwards, for a seal of radius r and clearance s:
//! - impeller seat: r − e_r (the ring sits on it), or r with no impeller ring;
//! - impeller ring: from the seat to r, its outer face being the wall of the gap;
//!   The neck wall t of the shroud stays under the ring, so the seat is never below
//!   d₁/2 + t: the automatic diameter is d_sp = d₁ + 2(t + e_r) with a wear ring and
//!   d₁ + 2t without it;
//! - casing ring: from r + s to the casing seat r + s + e_c; with no ring the casing
//!   itself bounds the gap and the seat is r + s.

use serde::{Deserialize, Serialize};

use super::ImpellerBasis;
use crate::Param;
use crate::losses::{SEAL_LENGTH_RATIO, seal_clearance};

/// Radial thickness of a wear ring over the seal diameter (our choice).
pub const RING_THICKNESS_RATIO: f64 = 0.03;
/// Least automatic radial thickness of a wear ring, m (our choice for a cast ring).
pub const MIN_RING_THICKNESS: f64 = 0.004;

/// Thickness of a wear ring unless given: 0.03·d, at least 4 mm.
pub fn default_ring_thickness(d: f64) -> f64 {
    (RING_THICKNESS_RATIO * d).max(MIN_RING_THICKNESS)
}

/// One replaceable wear ring.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RingSettings {
    pub enabled: bool,
    /// Radial thickness, m; automatic: [`default_ring_thickness`].
    pub thickness: Param<f64>,
}

impl Default for RingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            thickness: Param::Auto,
        }
    }
}

/// Inputs of one annular seal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SealSettings {
    pub enabled: bool,
    /// Diameter of the gap on the impeller side d_sp, m; automatic: d₁ + 2(t + e_r) with an
    /// impeller ring, d₁ + 2t without it (t the wall); the rear seal takes the front's.
    pub diameter: Param<f64>,
    /// Length L, m; automatic: 0.12·d_sp (Gülich Chap. 3.6.2).
    pub length: Param<f64>,
    /// Radial clearance s, m; automatic: Gülich eq. 3.12.
    pub clearance: Param<f64>,
    pub impeller_ring: RingSettings,
    pub casing_ring: RingSettings,
}

impl Default for SealSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            diameter: Param::Auto,
            length: Param::Auto,
            clearance: Param::Auto,
            impeller_ring: RingSettings::default(),
            casing_ring: RingSettings::default(),
        }
    }
}

/// Both seals of an impeller.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Seals {
    pub front: SealSettings,
    pub rear: SealSettings,
}

impl Seals {
    /// Seals of an earlier project: the front one by `neck_length` (a manual 0 turns it
    /// off), no rear one and no wear rings.
    pub fn legacy(neck_length: Param<f64>) -> Self {
        let off = RingSettings {
            enabled: false,
            ..RingSettings::default()
        };
        let front_on = !matches!(neck_length, Param::Manual(v) if v <= 0.0);
        Self {
            front: SealSettings {
                enabled: front_on,
                length: neck_length,
                impeller_ring: off,
                casing_ring: off,
                ..SealSettings::default()
            },
            rear: SealSettings {
                enabled: false,
                impeller_ring: off,
                casing_ring: off,
                ..SealSettings::default()
            },
        }
    }

    /// The seals as used for `basis` and a shroud of `wall_thickness`. `eyes` is 1 or 2
    /// ([`ImpellerBasis`] does not carry it); a double-entry impeller has no rear seal.
    pub fn resolve(&self, basis: &ImpellerBasis, wall_thickness: f64, eyes: f64) -> ResolvedSeals {
        let mut warnings = Vec::new();
        let d_base = basis.inlet_diameter + 2.0 * wall_thickness;
        let ring = default_ring_thickness(d_base);
        let front_d = self.front.diameter.resolve(|| {
            d_base
                + if self.front.impeller_ring.enabled {
                    2.0 * ring
                } else {
                    0.0
                }
        });
        let rear_d = self.rear.diameter.resolve(|| front_d);
        let front = resolve_seal(
            "front",
            &self.front,
            basis,
            front_d,
            ring,
            wall_thickness,
            &mut warnings,
        );
        let rear = if self.rear.enabled && eyes > 1.5 {
            warnings.push(
                "a double-entry impeller has no rear seal: the rear seal is left out".to_string(),
            );
            None
        } else {
            resolve_seal(
                "rear",
                &self.rear,
                basis,
                rear_d,
                ring,
                wall_thickness,
                &mut warnings,
            )
        };
        ResolvedSeals {
            front,
            rear,
            warnings,
        }
    }
}

fn resolve_seal(
    name: &str,
    s: &SealSettings,
    basis: &ImpellerBasis,
    d: f64,
    ring: f64,
    wall_thickness: f64,
    warnings: &mut Vec<String>,
) -> Option<Seal> {
    if !s.enabled {
        return None;
    }
    let radius = d / 2.0;
    let length = s.length.resolve(|| SEAL_LENGTH_RATIO * d);
    let min_clearance = seal_clearance(d);
    let clearance = s.clearance.resolve(|| min_clearance);
    if clearance < min_clearance * (1.0 - 1e-9) {
        warnings.push(format!(
            "the {name} seal clearance {:.1} mm is below the {:.1} mm Gülich recommends for a seal of {:.1} mm",
            clearance * 1e3,
            min_clearance * 1e3,
            d * 1e3
        ));
    }
    let impeller_ring = s.impeller_ring.enabled.then(|| {
        let wanted = s.impeller_ring.thickness.resolve(|| ring);
        let floor = basis.inlet_diameter / 2.0 + wall_thickness;
        let most = (radius - floor).max(0.0);
        if wanted > most + 1e-12 {
            warnings.push(format!(
                "the {name} impeller ring of {:.1} mm is trimmed to {:.1} mm to leave the neck a wall",
                wanted * 1e3,
                most * 1e3
            ));
            most
        } else {
            wanted
        }
    });
    let casing_ring = s
        .casing_ring
        .enabled
        .then(|| s.casing_ring.thickness.resolve(|| ring));
    Some(Seal {
        radius,
        length,
        clearance,
        impeller_seat: radius - impeller_ring.unwrap_or(0.0),
        casing_seat: radius + clearance + casing_ring.unwrap_or(0.0),
        impeller_ring,
        casing_ring,
    })
}

/// A seal as used; radii in m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seal {
    /// Outside of the impeller ring, in the gap.
    pub radius: f64,
    pub length: f64,
    pub clearance: f64,
    /// r − e_r, or r with no impeller ring.
    pub impeller_seat: f64,
    /// r + s + e_c, or r + s with no casing ring.
    pub casing_seat: f64,
    /// e_r.
    pub impeller_ring: Option<f64>,
    /// e_c.
    pub casing_ring: Option<f64>,
}

/// The seals as used, with what the design says about them.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSeals {
    pub front: Option<Seal>,
    pub rear: Option<Seal>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::impeller::meridian::tests::basis;

    // The automatic wall of a cast impeller of this basis.
    const WALL: f64 = 0.0034;

    #[test]
    fn default_seals_have_both_pairs_of_rings() {
        let b = basis();
        let r = Seals::default().resolve(&b, WALL, 1.0);
        let (front, rear) = (r.front.unwrap(), r.rear.unwrap());
        let e = default_ring_thickness(b.inlet_diameter + 2.0 * WALL);
        let seat = b.inlet_diameter / 2.0 + WALL;
        let radius = seat + e;
        assert!((front.radius - radius).abs() < 1e-12);
        assert_eq!(rear.radius, front.radius);
        assert_eq!(front.length, SEAL_LENGTH_RATIO * 2.0 * front.radius);
        assert_eq!(front.clearance, seal_clearance(2.0 * front.radius));
        assert!((front.impeller_seat - seat).abs() < 1e-12);
        assert!((front.casing_seat - (radius + front.clearance + e)).abs() < 1e-12);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn without_an_impeller_ring_the_seal_sits_on_the_wall() {
        let b = basis();
        let mut s = Seals::default();
        s.front.impeller_ring.enabled = false;
        let f = s.resolve(&b, WALL, 1.0).front.unwrap();
        assert!((f.radius - (b.inlet_diameter / 2.0 + WALL)).abs() < 1e-12);
        assert_eq!(f.impeller_seat, f.radius);
    }

    #[test]
    fn a_disabled_ring_leaves_the_part_solid_to_the_gap() {
        let b = basis();
        let mut s = Seals::default();
        s.front.impeller_ring.enabled = false;
        s.front.casing_ring.enabled = false;
        let f = s.resolve(&b, WALL, 1.0).front.unwrap();
        assert_eq!(f.impeller_seat, f.radius);
        assert_eq!(f.casing_seat, f.radius + f.clearance);
        assert!(f.impeller_ring.is_none() && f.casing_ring.is_none());
    }

    #[test]
    fn a_disabled_seal_is_none() {
        let b = basis();
        let mut s = Seals::default();
        s.front.enabled = false;
        s.rear.enabled = false;
        let r = s.resolve(&b, WALL, 1.0);
        assert!(r.front.is_none() && r.rear.is_none());
    }

    #[test]
    fn legacy_seals_keep_the_old_front_neck() {
        let b = basis();
        let r = Seals::legacy(Param::Manual(0.02)).resolve(&b, WALL, 1.0);
        let f = r.front.unwrap();
        assert_eq!(f.length, 0.02);
        assert!(f.impeller_ring.is_none() && f.casing_ring.is_none());
        assert!(r.rear.is_none());
        let off = Seals::legacy(Param::Manual(0.0)).resolve(&b, WALL, 1.0);
        assert!(off.front.is_none());
        let auto = Seals::legacy(Param::Auto).resolve(&b, WALL, 1.0);
        assert!(auto.front.is_some());
    }

    #[test]
    fn a_ring_too_thick_for_the_eye_is_trimmed_with_a_warning() {
        let b = basis();
        let mut s = Seals::default();
        s.front.impeller_ring.thickness = Param::Manual(0.5);
        let r = s.resolve(&b, WALL, 1.0);
        let f = r.front.unwrap();
        assert!((f.impeller_seat - (b.inlet_diameter / 2.0 + WALL)).abs() < 1e-12);
        assert!(f.impeller_ring.unwrap() > 0.0);
        assert!(!r.warnings.is_empty());
    }

    #[test]
    fn a_double_entry_impeller_has_no_rear_seal() {
        let b = basis();
        let r = Seals::default().resolve(&b, WALL, 2.0);
        assert!(r.rear.is_none());
        assert!(r.front.is_some());
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn a_clearance_below_guelich_minimum_warns() {
        let b = basis();
        let mut s = Seals::default();
        s.front.clearance = Param::Manual(1e-5);
        let r = s.resolve(&b, WALL, 1.0);
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("clearance"));
    }
}
