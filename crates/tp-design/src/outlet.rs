//! The flow leaving an impeller, as seen by the casing or diffuser after it.

use crate::pump::{CentrifugalPumpDesign, PumpDuty};

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
        }
    }
}
