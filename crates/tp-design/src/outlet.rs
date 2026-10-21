//! The flow leaving an impeller, as seen by the casing or diffuser after it.

use tp_math::DVec2;

use crate::pump::{CentrifugalPumpDesign, PumpDuty};

/// Points of each side profile of [`ImpellerSides`].
pub const SIDE_POINTS: usize = 17;

/// How far the impeller reaches axially on each side, for the clearance of a casing
/// beside it: its outer faces, hub disk and shrouds included, as points (r, z) from r₂
/// inward. Inside the innermost radius of a profile lies the suction eye, where the
/// inlet goes on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImpellerSides {
    /// Toward −z: the least z of the impeller at each radius.
    pub minus: [DVec2; SIDE_POINTS],
    /// Toward +z: the greatest z of the impeller at each radius.
    pub plus: [DVec2; SIDE_POINTS],
}

/// Where the impeller reaches over a range of radii on one side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reach {
    /// The range lies outside the impeller.
    Clear,
    /// Its farthest z over the range, m.
    At(f64),
    /// The range enters the suction eye.
    Eye,
}

impl ImpellerSides {
    /// How far the impeller reaches toward `toward` (+1: +z, −1: −z) over radii from
    /// `r_lo` to `r_hi`: the greatest z toward +z, the least toward −z.
    pub fn farthest(&self, toward: f64, r_lo: f64, r_hi: f64) -> Reach {
        let side = if toward > 0.0 {
            &self.plus
        } else {
            &self.minus
        };
        let (outer, inner) = (side[0].x, side[SIDE_POINTS - 1].x);
        let (lo, hi) = (r_lo.max(inner), r_hi.min(outer));
        if r_lo < inner - 1e-12 && r_hi > inner {
            return Reach::Eye;
        }
        if lo > hi {
            return Reach::Clear;
        }
        // z at radius r, from the profile ordered by decreasing radius.
        let at = |r: f64| {
            let k = side
                .windows(2)
                .position(|w| r <= w[0].x && r >= w[1].x)
                .unwrap_or(SIDE_POINTS - 2);
            let (a, b) = (side[k], side[k + 1]);
            let t = if (a.x - b.x).abs() > 0.0 {
                (a.x - r) / (a.x - b.x)
            } else {
                0.0
            };
            a.y + (b.y - a.y) * t
        };
        let farther = |a: f64, b: f64| if toward > 0.0 { a.max(b) } else { a.min(b) };
        let ends = farther(at(lo), at(hi));
        Reach::At(
            side.iter()
                .filter(|p| p.x > lo && p.x < hi)
                .fold(ends, |z, p| farther(z, p.y)),
        )
    }
}

/// What a casing or diffuser receives from the impeller just upstream, in SI units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImpellerOutlet {
    /// Flow through the machine at the design point, m³/s. The leakage at the impeller
    /// eye returns to the suction side and does not pass the volute.
    pub flow: f64,
    /// Impeller outlet diameter d₂, m.
    pub impeller_diameter: f64,
    /// Outlet width of the whole impeller b₂, m.
    pub impeller_width: f64,
    /// Circumferential velocity at the impeller outlet c₂u, m/s.
    pub cu: f64,
    /// Meridional velocity at the impeller outlet c₂m, m/s.
    pub cm: f64,
    /// Specific speed of the stage.
    pub nq: f64,
    /// Head of the stage, m.
    pub stage_head: f64,
    /// kg/m³
    pub density: f64,
    /// Axial position of the midplane of the impeller outlet, m.
    pub midplane: f64,
    /// +1 when the impeller turns in the positive sense about +z, −1 otherwise.
    pub rotation_sign: f64,
    /// Number of impeller blades z_La, for the vane-number rules of diffusers.
    pub impeller_blades: u32,
    /// The outer faces of the impeller beside the casing, when it is the impeller that
    /// comes just before.
    pub sides: Option<ImpellerSides>,
}

impl ImpellerOutlet {
    /// The outlet of a radial pump impeller.
    pub fn from_pump(
        design: &CentrifugalPumpDesign,
        duty: &PumpDuty,
        midplane: f64,
        rotation_sign: f64,
    ) -> Self {
        Self {
            flow: duty.flow,
            impeller_diameter: design.outlet_diameter,
            impeller_width: design.outlet_width,
            cu: design.outlet.triangle.cu,
            cm: design.outlet.triangle.cm,
            nq: design.nq,
            stage_head: duty.head,
            density: duty.density,
            midplane,
            rotation_sign,
            impeller_blades: design.blade_count,
            sides: None,
        }
    }
}
