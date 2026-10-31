//! The impeller and the volute as the CAD model builds them, for the 3D view: generated
//! when asked, in the background, each shown as soon as it is ready, and only while the
//! design is the one they were built from. The view never waits for them: until then,
//! and as soon as the design changes, it shows its own mesh.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
use tp_design::impeller::ImpellerGeometry;
use tp_design::volute::VoluteGeometry;

use super::mesh::CadMesh;

/// One part of the CAD model, built.
#[cfg_attr(not(feature = "cad"), allow(dead_code))]
#[derive(Debug)]
enum Piece {
    /// The flow domain and the casing, and whether the flow domain was sewn.
    Volute(Result<(CadMesh, bool, Vec<String>), String>),
    /// The impeller's material: disk, cover and blades.
    Impeller(Result<(CadMesh, Vec<String>), String>),
}

/// What the CAD model gave the view.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Built {
    pub mesh: CadMesh,
    /// The flow domain of the volute was sewn from its surfaces, not joined by booleans.
    pub sewn: Option<bool>,
    /// How long the model took, s.
    pub seconds: f64,
    /// What the CAD kernel could not build as designed, and the parts it could not
    /// build at all.
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
        job: Receiver<Piece>,
        /// Parts still to come.
        pending: usize,
        built: Built,
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

/// Where the CAD model stands, for the design at a revision.
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

/// The CAD model between frames.
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

    /// Starts the model asked for, of the impeller `impeller` and the volute `volute` at
    /// `revision`, and takes in the parts that finished.
    pub(crate) fn update(
        &mut self,
        revision: u64,
        impeller: &ImpellerGeometry,
        volute: Option<&VoluteGeometry>,
        ctx: &egui::Context,
    ) {
        if std::mem::take(&mut self.requested) {
            self.state = start(revision, impeller.clone(), volute.cloned());
            self.generation += 1;
        }
        let State::Building {
            revision,
            started,
            job,
            pending,
            built,
        } = &mut self.state
        else {
            return;
        };
        loop {
            match job.try_recv() {
                Ok(piece) => {
                    *pending -= 1;
                    self.generation += 1;
                    match piece {
                        Piece::Volute(Ok((mesh, sewn, warnings))) => {
                            built.mesh.flow = mesh.flow;
                            built.mesh.casing = mesh.casing;
                            built.sewn = Some(sewn);
                            built.warnings.extend(warnings);
                        }
                        Piece::Impeller(Ok((mesh, warnings))) => {
                            built.mesh.impeller = mesh.impeller;
                            built.warnings.extend(warnings);
                        }
                        Piece::Volute(Err(e)) | Piece::Impeller(Err(e)) => built.warnings.push(e),
                    }
                }
                Err(TryRecvError::Empty) if *pending > 0 => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    return;
                }
                Err(_) => break,
            }
            if *pending == 0 {
                break;
            }
        }
        let revision = *revision;
        let mut built = std::mem::take(built);
        built.seconds = started.elapsed().as_secs_f64();
        self.state = if built.mesh == CadMesh::default() {
            State::Failed {
                revision,
                error: built.warnings.join("; "),
            }
        } else {
            State::Ready { revision, built }
        };
        self.generation += 1;
    }

    /// The parts to show for the design at `revision`, as they come: none if they were
    /// built for another.
    pub(crate) fn mesh(&self, revision: u64) -> Option<&CadMesh> {
        match &self.state {
            State::Building {
                revision: r, built, ..
            }
            | State::Ready { revision: r, built }
                if *r == revision =>
            {
                Some(&built.mesh)
            }
            _ => None,
        }
    }

    pub(crate) fn status(&self, revision: u64) -> Status<'_> {
        match &self.state {
            State::None => Status::Absent,
            State::Building {
                revision: r,
                started,
                ..
            } if *r == revision => Status::Building(started.elapsed().as_secs_f32()),
            State::Ready { revision: r, built } if *r == revision => Status::Ready(built),
            State::Failed { revision: r, error } if *r == revision => Status::Failed(error),
            _ => Status::Stale,
        }
    }
}

/// Builds the CAD model of the impeller `impeller` and the volute `volute` in the
/// background, each on its own thread.
#[cfg(feature = "cad")]
fn start(revision: u64, impeller: ImpellerGeometry, volute: Option<VoluteGeometry>) -> State {
    let (send, job) = std::sync::mpsc::channel();
    let mut pending = 1;
    if let Some(g) = volute {
        pending += 1;
        let send = send.clone();
        std::thread::spawn(move || {
            let _ = send.send(Piece::Volute(build_volute(&g)));
        });
    }
    std::thread::spawn(move || {
        let _ = send.send(Piece::Impeller(build_impeller(&impeller)));
    });
    State::Building {
        revision,
        started: Instant::now(),
        job,
        pending,
        built: Built::default(),
    }
}

#[cfg(not(feature = "cad"))]
fn start(revision: u64, _impeller: ImpellerGeometry, _volute: Option<VoluteGeometry>) -> State {
    State::Failed {
        revision,
        error: "this build has no CAD kernel".to_owned(),
    }
}

/// Triangles of `shape` (mm) about as fine as the view needs, `diameter` (m) over 600,
/// in metres.
#[cfg(feature = "cad")]
fn triangles(shape: &tp_cad::Shape, diameter: f64) -> Result<Vec<tp_io::mesh::Triangle>, String> {
    Ok(shape
        .triangles(diameter * 1e3 / 600.0)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| t.map(|p| p / 1e3))
        .collect())
}

/// The flow domain (sewn where it closes, else by booleans) and the casing without its
/// flange, which adds nothing to the flow.
#[cfg(feature = "cad")]
fn build_volute(g: &VoluteGeometry) -> Result<(CadMesh, bool, Vec<String>), String> {
    let (cad, how) = tp_cad::build_volute_flow(g).map_err(|e| e.to_string())?;
    let d = g.basis.impeller_diameter;
    let flow = triangles(&cad.fluid, d)?;
    let mut bare = g.clone();
    bare.design.flange = None;
    let mut warnings = cad.warnings.clone();
    let casing = match tp_cad::build_volute_casing(&bare, &cad) {
        Ok(casing) => {
            warnings.extend(casing.warnings.iter().cloned());
            triangles(&casing.solid, d)?
        }
        Err(e) => {
            warnings.push(e.to_string());
            Vec::new()
        }
    };
    let mesh = CadMesh {
        flow,
        casing,
        ..CadMesh::default()
    };
    Ok((mesh, how == tp_cad::FlowBuild::Sewn, warnings))
}

/// The impeller's material as the CAD export builds it: disk, cover, blades and the
/// fillets at their roots.
#[cfg(feature = "cad")]
fn build_impeller(g: &ImpellerGeometry) -> Result<(CadMesh, Vec<String>), String> {
    let cad =
        tp_cad::build_impeller(g, &tp_cad::CadOptions::default()).map_err(|e| e.to_string())?;
    let mesh = CadMesh {
        impeller: triangles(&cad.impeller, g.basis.outlet_diameter)?,
        ..CadMesh::default()
    };
    Ok((mesh, cad.warnings))
}
