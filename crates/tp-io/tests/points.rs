//! The point list can be read back and reproduces the geometry.

use std::f64::consts::PI;

use tp_design::impeller::{self, ImpellerBasis, ImpellerGeometryInput};
use tp_design::pump::{CentrifugalPumpInput, PumpDuty, design};
use tp_io::points::geometry_txt;

#[test]
fn point_list_round_trip() {
    let duty = PumpDuty {
        flow: 100.0 / 3600.0,
        head: 32.0,
        machine_head: 32.0,
        stages: 1,
        speed: 2900.0 * PI / 30.0,
        density: 998.2,
        vapor_pressure: 2339.0,
        inlet_total_pressure: 101_325.0,
        kinematic_viscosity: 1.0e-6,
    };
    let input = CentrifugalPumpInput::default();
    let main = design(&duty, &input).unwrap();
    let basis = ImpellerBasis::from_pump(&main, &input, 1.0);
    let g = impeller::build(&basis, &ImpellerGeometryInput::default()).unwrap();
    let text = geometry_txt(&g);

    let mut section = String::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            section = line.to_owned();
            continue;
        }
        let values: Vec<f64> = line.split(' ').map(|v| v.parse().unwrap()).collect();
        if section.starts_with("[section span=1.0000]") {
            rows.push(values);
        }
    }
    let shroud = g.blade.sections.last().unwrap();
    assert_eq!(rows.len(), shroud.m.len());
    for (row, k) in rows.iter().zip(0..) {
        assert_eq!(row.len(), 15);
        assert!((row[2] - shroud.meridional[k].y).abs() < 1e-12 * row[2].abs().max(1.0));
        assert!((row[3] - shroud.theta[k]).abs() < 1e-11);
        // Mean-line point is on the radius r.
        let r = (row[6] * row[6] + row[7] * row[7]).sqrt();
        assert!((r - row[2]).abs() < 1e-9);
    }
}
