//! Prints the blade geometry of the 100 m³/h, 32 m, 2900 rpm reference pump.

use std::f64::consts::PI;

use tp_design::impeller::{self, ImpellerBasis, ImpellerGeometryInput};
use tp_design::pump::{CentrifugalPumpInput, PumpDuty, design};

fn main() {
    let duty = PumpDuty {
        flow: 100.0 / 3600.0,
        head: 32.0,
        machine_head: 32.0,
        stages: 1,
        speed: 2900.0 * PI / 30.0,
        density: 998.2,
        vapor_pressure: 2339.0,
        inlet_total_pressure: 101_325.0,
    };
    let input = CentrifugalPumpInput::default();
    let main = design(&duty, &input).expect("feasible main dimensions");
    let basis = ImpellerBasis::from_pump(&main, &input, 1.0);
    let g = impeller::build(&basis, &ImpellerGeometryInput::default()).expect("geometry");

    println!("axial length {:.1} mm", g.contour.axial_length * 1e3);
    let qos = &g.channel.quasi_orthogonals;
    for q in qos.iter().step_by(10) {
        println!(
            "t {:.2}  area {:.2} cm²  cm {:.2} m/s",
            q.t,
            q.area * 1e4,
            q.cm
        );
    }
    for s in &g.blade.sections {
        let le = &s.leading_edge;
        println!(
            "span {:.2}: r_LE {:.1} mm  cm {:.2}  β1 {:.1}°  β1B {:.1}°  wrap {:.1}°  length {:.1} mm",
            s.span,
            le.position.y * 1e3,
            le.cm,
            le.flow_angle.to_degrees(),
            le.blade_angle.to_degrees(),
            s.wrap_angle().to_degrees(),
            s.mean_line_length() * 1e3
        );
    }
    println!(
        "blade loading ξ_eff {:.3}  ξ_al {:.3}  ratio {:.2}",
        g.blade_loading.effective,
        g.blade_loading.allowable,
        g.blade_loading.ratio()
    );
    println!("warnings: {:?}", g.warnings);
}
