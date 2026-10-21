//! DWG through the ODA File Converter, a free program of the Open Design Alliance that
//! the user installs (<https://www.opendesign.com/guestfiles/oda_file_converter>):
//! Turbopump writes the drawing as DXF and the converter saves it as DWG. Neither the
//! converter nor any DWG code ships with Turbopump.
//!
//! Unlike the other writers, this module touches the file system and runs a program:
//! the one [`CONVERTER_ENV`] names, or else the converter where its installers put it.
//! A path read from a project file is never run.

use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Where the ODA File Converter is downloaded.
pub const DOWNLOAD_URL: &str = "https://www.opendesign.com/guestfiles/oda_file_converter";
/// Environment variable with the path of the converter's executable.
pub const CONVERTER_ENV: &str = "TURBOPUMP_ODAFC";
/// Longest wait for one conversion.
const TIMEOUT: Duration = Duration::from_secs(120);

/// DWG file format, named by the AutoCAD release that introduced it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DwgVersion {
    R2000,
    R2004,
    R2007,
    R2010,
    R2013,
    #[default]
    R2018,
}

impl DwgVersion {
    pub const ALL: [DwgVersion; 6] = [
        DwgVersion::R2000,
        DwgVersion::R2004,
        DwgVersion::R2007,
        DwgVersion::R2010,
        DwgVersion::R2013,
        DwgVersion::R2018,
    ];

    pub fn year(self) -> u16 {
        match self {
            DwgVersion::R2000 => 2000,
            DwgVersion::R2004 => 2004,
            DwgVersion::R2007 => 2007,
            DwgVersion::R2010 => 2010,
            DwgVersion::R2013 => 2013,
            DwgVersion::R2018 => 2018,
        }
    }
}

impl fmt::Display for DwgVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.year())
    }
}

impl FromStr for DwgVersion {
    type Err = String;

    /// `2018`, `R2018` or `ACAD2018`, in any case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let upper = s.trim().to_ascii_uppercase();
        let year = upper
            .strip_prefix("ACAD")
            .or_else(|| upper.strip_prefix('R'))
            .unwrap_or(&upper);
        DwgVersion::ALL
            .into_iter()
            .find(|v| v.year().to_string() == year)
            .ok_or_else(|| {
                let years: Vec<String> = DwgVersion::ALL.iter().map(|v| v.to_string()).collect();
                format!("DWG version must be one of {}, got {s:?}", years.join(", "))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DwgError {
    /// No converter where [`find_converter`] looks.
    NotInstalled,
    Io(String),
    /// The converter ran but made no DWG; its own messages, if it wrote any.
    Failed(String),
}

impl fmt::Display for DwgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DwgError::NotInstalled => write!(
                f,
                "DWG export needs the free ODA File Converter ({DOWNLOAD_URL}); install it, \
                 or set {CONVERTER_ENV} to its executable"
            ),
            DwgError::Io(e) => f.write_str(e),
            DwgError::Failed(e) => write!(f, "the ODA File Converter failed: {e}"),
        }
    }
}

impl std::error::Error for DwgError {}

/// The converter's executable: the file [`CONVERTER_ENV`] names when it is set, or else
/// the first found where the installers put it and on the `PATH`.
pub fn find_converter() -> Option<PathBuf> {
    converter_in(
        std::env::var_os(CONVERTER_ENV).as_deref(),
        &installed(),
        std::env::var_os("PATH").as_deref(),
    )
}

fn converter_in(
    env: Option<&OsStr>,
    installed: &[PathBuf],
    path: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(env) = env {
        // An explicit choice is not second-guessed: if it is wrong, say so.
        let file = PathBuf::from(env);
        return file.is_file().then_some(file);
    }
    let name = if cfg!(windows) {
        "ODAFileConverter.exe"
    } else {
        "ODAFileConverter"
    };
    let on_path = path
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join(name));
    installed
        .iter()
        .cloned()
        .chain(on_path)
        .find(|p| p.is_file())
}

/// Where the converter's installers put it on this system.
fn installed() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        let app = "ODAFileConverter.app/Contents/MacOS/ODAFileConverter";
        let mut places = vec![Path::new("/Applications").join(app)];
        if let Some(home) = std::env::var_os("HOME") {
            places.push(Path::new(&home).join("Applications").join(app));
        }
        places
    } else if cfg!(windows) {
        // C:\Program Files\ODA\ODAFileConverter <version>\ODAFileConverter.exe; the newest
        // version first.
        let mut bases: Vec<PathBuf> = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .into_iter()
            .collect();
        bases.push(PathBuf::from(r"C:\Program Files"));
        let mut places = Vec::new();
        for base in bases {
            let Ok(entries) = std::fs::read_dir(base.join("ODA")) else {
                continue;
            };
            let mut dirs: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("ODAFileConverter")
                })
                .map(|e| e.path().join("ODAFileConverter.exe"))
                .collect();
            dirs.sort();
            places.extend(dirs.into_iter().rev());
        }
        places
    } else {
        [
            "/usr/bin/ODAFileConverter",
            "/usr/local/bin/ODAFileConverter",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    }
}

/// Saves `dxf`, the text of a DXF document, as the DWG file `path` in `version`, with
/// the converter `converter`.
pub fn write(
    converter: &Path,
    dxf: &str,
    path: &Path,
    version: DwgVersion,
) -> Result<(), DwgError> {
    // The converter works on folders: it converts every DXF of `input` into `output`.
    let work = WorkDir::new()?;
    let input = work.0.join("in");
    let output = work.0.join("out");
    for dir in [&input, &output] {
        std::fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;
    }
    let source = input.join("drawing.dxf");
    std::fs::write(&source, dxf).map_err(|e| io_error(&source, e))?;

    let mut child = Command::new(converter)
        .arg(&input)
        .arg(&output)
        .arg(format!("ACAD{}", version.year()))
        .arg("DWG")
        .arg("0") // not recursive
        .arg("1") // audit each file
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| io_error(converter, e))?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| io_error(converter, e))? {
            break status;
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(DwgError::Failed(format!(
                "no answer after {} s",
                TIMEOUT.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let files: Vec<PathBuf> = std::fs::read_dir(&output)
        .map_err(|e| io_error(&output, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    let has = |p: &PathBuf, ext: &str| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext));
    // It reports each file it could not convert in a .err file of the same name.
    let messages: Vec<String> = files
        .iter()
        .filter(|p| has(p, "err"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .map(|m| m.trim().to_owned())
        .collect();
    if !messages.is_empty() {
        return Err(DwgError::Failed(messages.join("\n")));
    }
    if !status.success() {
        return Err(DwgError::Failed(format!("it ended with {status}")));
    }
    let Some(dwg) = files.iter().find(|p| has(p, "dwg")) else {
        return Err(DwgError::Failed(
            "it wrote no DWG file (on Linux without a display it needs a virtual one, \
             such as xvfb-run)"
                .into(),
        ));
    };
    std::fs::copy(dwg, path).map_err(|e| io_error(path, e))?;
    Ok(())
}

fn io_error(path: &Path, e: std::io::Error) -> DwgError {
    DwgError::Io(format!("{}: {e}", path.display()))
}

/// A new folder in the system's temporary folder, removed with everything in it when
/// dropped.
struct WorkDir(PathBuf);

impl WorkDir {
    fn new() -> Result<Self, DwgError> {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "turbopump-dwg-{}-{nanos}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
        Ok(Self(dir))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_in_every_spelling() {
        for text in ["2013", "R2013", "acad2013", " ACAD2013 "] {
            assert_eq!(text.parse::<DwgVersion>(), Ok(DwgVersion::R2013));
        }
        assert!("2019".parse::<DwgVersion>().is_err());
        assert_eq!(DwgVersion::default().to_string(), "2018");
    }

    #[test]
    fn not_installed_says_where_to_get_it() {
        let message = DwgError::NotInstalled.to_string();
        assert!(message.contains(DOWNLOAD_URL));
        assert!(message.contains(CONVERTER_ENV));
    }

    #[cfg(unix)]
    mod with_a_stand_in {
        //! A shell script with the converter's arguments stands in for it.

        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Mutex;

        /// Writing a script while another test starts a process can leave the script
        /// busy for exec (ETXTBSY): one test at a time.
        static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

        fn script(dir: &Path, body: &str) -> PathBuf {
            let file = dir.join("ODAFileConverter");
            std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).expect("script");
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
                .expect("executable");
            file
        }

        #[test]
        fn passes_folders_version_and_flags() {
            let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
            let dir = WorkDir::new().expect("work dir");
            // Echo the arguments, then "convert" by copying.
            let converter = script(
                &dir.0,
                r#"printf '%s %s %s %s\n' "$3" "$4" "$5" "$6" > "$2/drawing.dwg"
cat "$1/drawing.dxf" >> "$2/drawing.dwg""#,
            );
            let target = dir.0.join("meridional.dwg");
            write(&converter, "0\nEOF\n", &target, DwgVersion::R2013).expect("converted");
            assert_eq!(
                std::fs::read_to_string(&target).expect("dwg"),
                "ACAD2013 DWG 0 1\n0\nEOF\n"
            );
        }

        #[test]
        fn reports_the_converters_error_file() {
            let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
            let dir = WorkDir::new().expect("work dir");
            let converter = script(
                &dir.0,
                r#"echo "Unknown entity in line 12" > "$2/drawing.err""#,
            );
            let error = write(&converter, "", &dir.0.join("x.dwg"), DwgVersion::R2018)
                .expect_err("error file");
            assert_eq!(error, DwgError::Failed("Unknown entity in line 12".into()));
        }

        #[test]
        fn a_run_without_output_is_an_error() {
            let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
            let dir = WorkDir::new().expect("work dir");
            let silent = script(&dir.0, "exit 0");
            let error = write(&silent, "", &dir.0.join("x.dwg"), DwgVersion::R2018)
                .expect_err("nothing written");
            assert!(matches!(error, DwgError::Failed(m) if m.contains("no DWG")));
            let failing = script(&dir.0, "exit 3");
            assert!(matches!(
                write(&failing, "", &dir.0.join("x.dwg"), DwgVersion::R2018),
                Err(DwgError::Failed(_))
            ));
        }

        #[test]
        fn finds_the_converter_by_variable_install_place_or_path() {
            let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
            let dir = WorkDir::new().expect("work dir");
            let converter = script(&dir.0, "exit 0");
            let missing = dir.0.join("missing");

            assert_eq!(
                converter_in(Some(converter.as_os_str()), &[], None),
                Some(converter.clone())
            );
            // A wrong variable is not replaced by another converter.
            assert_eq!(
                converter_in(
                    Some(missing.as_os_str()),
                    std::slice::from_ref(&converter),
                    Some(dir.0.as_os_str())
                ),
                None
            );
            assert_eq!(
                converter_in(None, &[missing.clone(), converter.clone()], None),
                Some(converter.clone())
            );
            assert_eq!(
                converter_in(None, &[missing], Some(dir.0.as_os_str())),
                Some(converter)
            );
            assert_eq!(converter_in(None, &[], None), None);
        }
    }
}
