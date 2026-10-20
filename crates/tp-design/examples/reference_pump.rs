//! Prints the main dimensions of a 100 m³/h, 32 m, 2900 rpm water pump.

use std::f64::consts::PI;

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
    let d = design(&duty, &CentrifugalPumpInput::default()).expect("feasible design");
    println!("{d:#?}");
}
