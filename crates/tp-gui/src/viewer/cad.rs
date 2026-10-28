//! The volute as the CAD model builds it, for the 3D view: generated when asked, in the
//! background, and shown only while the design is the one it was built from. The view
//! never waits for it: until it is ready, and as soon as the design changes, it shows
//! its own mesh.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
use tp_design::volute::VoluteGeometry;

use super::mesh::CadMesh;

/// What the CAD model of the volute gave the view.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Built {
    pub mesh: CadMesh,
    /// The flow domain was sewn from its surfaces, not joined by booleans.
    pub sewn: bool,
    /// How long the model took, s.
    pub seconds: f64,
    /// What the CAD kernel could not build as designed.
    pub warnings: Vec<String>,
}

#[derive(Debug, Default)]
enum State {
    #[default]
    None,
    #[cfg_attr(not(feature = "cad"), allow(dead_code))]
    Building {
        revision: u64,
        started: Instant,
        job: Receiver<Result<Built, String>>,
    },
    Ready {
        revision: u64,
        built: Built,
    },
    Failed {
        revision: u64,
        error: String,
    },
}

/// Where the CAD model of the volute stands, for the design at a revision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Status<'a> {
    /// Never asked for.
    Absent,
    /// Being built, for this many seconds.
    Building(f32),
    /// Built for another design.
    Stale,
    Ready(&'a Built),
    Failed(&'a str),
}

/// The CAD model of the volute between frames.
#[derive(Debug, Default)]
pub(crate) struct CadView {
    state: State,
    /// Asked for since the last frame.
    requested: bool,
    /// Changes whenever the model shown changes, so the view uploads it again.
    generation: u64,
}

impl CadView {
    /// Whether this build can make the CAD model at all.
    pub(crate) const AVAILABLE: bool = cfg!(feature = "cad");

    /// Asks for the CAD model of the design shown next frame.
    pub(crate) fn request(&mut self) {
        self.requested = true;
    }

    /// Whether a model is being built.
    pub(crate) fn building(&self) -> bool {
        matches!(self.state, State::Building { .. })
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// Starts the model asked for, of the volute `volute` at `revision`, and takes in
    /// one that finished.
    pub(crate) fn update(
        &mut self,
        revision: u64,
        volute: Option<&VoluteGeometry>,
        ctx: &egui::Context,
    ) {
        if std::mem::take(&mut self.requested)
            && let Some(g) = volute
        {
            self.state = start(revision, g.clone());
            self.generation += 1;
        }
        if let State::Building { revision, job, .. } = &self.state {
            let revision = *revision;
            match job.try_recv() {
                Ok(Ok(built)) => self.state = State::Ready { revision, built },
                Ok(Err(error)) => self.state = State::Failed { revision, error },
                Err(TryRecvError::Disconnected) => {
                    self.state = State::Failed {
                        revision,
                        error: "the CAD model stopped".to_owned(),
                    }
                }
                Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(200)),
            }
            if !self.building() {
                self.generation += 1;
            }
        }
    }

    /// The model to show for the design at `revision`: none if it was built for
    /// another.
    pub(crate) fn mesh(&self, revision: u64) -> Option<&CadMesh> {
        match &self.state {
            State::Ready { revision: r, built } if *r == revision => Some(&built.mesh),
            _ => None,
        }
    }

    pub(crate) fn status(&self, revision: u64) -> Status<'_> {
        match &self.state {
            State::None => Status::Absent,
            State::Building { started, .. } => Status::Building(started.elapsed().as_secs_f32()),
            State::Ready { revision: r, built } if *r == revision => Status::Ready(built),
            State::Failed { revision: r, error } if *r == revision => Status::Failed(error),
            _ => Status::Stale,
        }
    }
}

/// Builds the CAD model of `g` in the background.
#[cfg(feature = "cad")]
fn start(revision: u64, g: VoluteGeometry) -> State {
    let (send, job) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(build(&g));
    });
    State::Building {
        revision,
        started: Instant::now(),
        job,
    }
}

#[cfg(not(feature = "cad"))]
fn start(revision: u64, _g: VoluteGeometry) -> State {
    State::Failed {
        revision,
        error: "this build has no CAD kernel".to_owned(),
    }
}

/// The flow domain (sewn where it closes, else by booleans) and the casing without its
/// flange, which adds nothing to the flow, triangulated about as finely as the view
/// needs: 1/600 of the impeller diameter.
#[cfg(feature = "cad")]
fn build(g: &VoluteGeometry) -> Result<Built, String> {
    use tp_math::DVec3;
    let started = Instant::now();
    let (cad, how) = tp_cad::build_volute_flow(g).map_err(|e| e.to_string())?;
    let deflection = g.basis.impeller_diameter * 1e3 / 600.0;
    let metres = |t: Vec<[DVec3; 3]>| -> Vec<[DVec3; 3]> {
        t.into_iter().map(|t| t.map(|p| p / 1e3)).collect()
    };
    let flow = metres(cad.fluid.triangles(deflection).map_err(|e| e.to_string())?);
    let mut bare = g.clone();
    bare.design.flange = None;
    let mut warnings = cad.warnings.clone();
    let casing = match tp_cad::build_volute_casing(&bare, &cad) {
        Ok(casing) => {
            warnings.extend(casing.warnings.iter().cloned());
            metres(
                casing
                    .solid
                    .triangles(deflection)
                    .map_err(|e| e.to_string())?,
            )
        }
        Err(e) => {
            warnings.push(e.to_string());
            Vec::new()
        }
    };
    Ok(Built {
        mesh: CadMesh { flow, casing },
        sewn: how == tp_cad::FlowBuild::Sewn,
        seconds: started.elapsed().as_secs_f64(),
        warnings,
    })
}
