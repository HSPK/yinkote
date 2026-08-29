//! Handing a PDF to something better at reading it.
//!
//! The built-in extractor is a text-layer reader: fast, offline, no model, and
//! very good at prose, which is what a summary and a close reading are made
//! of. Measured on real papers it recovers paragraphs faithfully in about
//! 100ms. It has two honest weaknesses:
//!
//! - **A scan has no text layer**, so it recovers nothing. Detected, not
//!   guessed at, by [`Extracted::is_useful`].
//! - **Tables and superscripts flatten.** A results table becomes rows of
//!   numbers with the column headings on a line of their own, and `10²⁰`
//!   becomes `10 20`. Prose is unaffected; a model asked to quote a table
//!   might misattribute a figure.
//!
//! The obvious answer is a layout model — Marker, MinerU, PaddleOCR. The
//! reason none is built in is that they are Python programs with a gigabyte or
//! two of weights and want a GPU to be quick, and this program is a single
//! binary somebody installs and runs. Bundling one would trade the premise of
//! the product for a better answer on a minority of files.
//!
//! So it is a seam rather than a dependency. Anything that takes a path and
//! prints text can be named in the configuration, and the built-in reader
//! stays the default — including as the fallback for when the external one is
//! not installed, which is the state every machine starts in.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use yk_core::{Error, Result};

use crate::Extracted;

/// When to reach for the external reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Never; the built-in reader only. The default, and what a machine with
    /// nothing installed does.
    #[default]
    Off,
    /// Only when the built-in reader found nothing worth having — a scan.
    /// The common setting: pages of images are the case it cannot do at all.
    Fallback,
    /// Always, falling back to the built-in reader if it fails. For somebody
    /// who cares about tables and has the model installed.
    Always,
}

impl Mode {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "fallback" | "auto" => Mode::Fallback,
            "always" => Mode::Always,
            _ => Mode::Off,
        }
    }
}

/// A reader that is another program.
#[derive(Debug, Clone)]
pub struct External {
    /// The program, e.g. `marker_single` or `python`.
    pub command: String,
    /// Its arguments. One must be `{}`, which becomes the path to the file.
    ///
    /// A placeholder rather than "the path goes last", because the programs
    /// worth naming here disagree about that: `marker_single FILE --output`
    /// takes it first.
    pub args: Vec<String>,
    /// How long to wait. A layout model on a CPU can take minutes on a long
    /// paper, and a summary nobody gets is worse than a rough one.
    pub timeout: Duration,
}

impl External {
    /// Whether the placeholder is present, which is the one way to get this
    /// wrong that produces no error of its own: without it the program is run
    /// on nothing and reports success on an empty document.
    pub fn names_the_file(&self) -> bool {
        self.args.iter().any(|a| a.contains("{}"))
    }

    /// Run it over a file already on disk.
    pub async fn read(&self, path: &Path) -> Result<String> {
        if !self.names_the_file() {
            return Err(Error::invalid(
                "the pdf command must include {} where the file's path goes",
            ));
        }
        let args: Vec<String> = self
            .args
            .iter()
            .map(|a| a.replace("{}", &path.to_string_lossy()))
            .collect();

        let child = tokio::process::Command::new(&self.command)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| Error::invalid(format!("could not run {}: {e}", self.command)))?;

        let finished = tokio::time::timeout(self.timeout, child.wait_with_output())
            .await
            .map_err(|_| {
                Error::Unavailable(format!(
                    "{} did not finish within {}s",
                    self.command,
                    self.timeout.as_secs()
                ))
            })?
            .map_err(|e| Error::invalid(format!("{} failed: {e}", self.command)))?;

        if !finished.status.success() {
            // The program's own last words, which is the only thing that says
            // whether the model is missing or the file was the problem.
            let said = String::from_utf8_lossy(&finished.stderr);
            let tail: String = said.lines().rev().take(3).collect::<Vec<_>>().join(" ");
            return Err(Error::invalid(format!("{} failed: {tail}", self.command)));
        }
        Ok(String::from_utf8_lossy(&finished.stdout).into_owned())
    }
}

/// The built-in reader, plus an external one when there is one.
#[derive(Debug, Clone, Default)]
pub struct Pipeline {
    pub mode: Mode,
    pub external: Option<External>,
}

impl Pipeline {
    /// Read a PDF held in memory.
    ///
    /// Never returns the external reader's failure as the answer: an extractor
    /// that is not installed, or that fell over on one file, must not turn a
    /// paper the built-in reader handles perfectly into an error. The reason
    /// is logged and the built-in result stands.
    pub async fn read(&self, bytes: &[u8]) -> Result<Extracted> {
        let external = match (self.mode, &self.external) {
            (Mode::Off, _) | (_, None) => None,
            (mode, Some(e)) => Some((mode, e)),
        };

        // `Always` still runs the built-in reader first. It costs ~100ms, and
        // it is what decides whether the external one is worth waiting for.
        //
        // On a blocking thread: it is CPU-bound for long enough to matter to
        // every other request sharing the runtime.
        let owned = bytes.to_vec();
        let builtin = tokio::task::spawn_blocking(move || crate::extract(&owned))
            .await
            .unwrap_or_else(|e| Err(Error::internal(format!("reading the file panicked: {e}"))));

        let Some((mode, external)) = external else {
            return builtin;
        };
        let good_enough = builtin.as_ref().is_ok_and(Extracted::is_useful);
        if mode == Mode::Fallback && good_enough {
            return builtin;
        }

        match self.run_external(external, bytes).await {
            Ok(got) if got.is_useful() => Ok(got),
            Ok(_) => {
                tracing::debug!(command = %external.command, "the external reader found no text");
                builtin
            }
            Err(e) => {
                tracing::warn!(error = %e, command = %external.command, "the external reader failed");
                builtin
            }
        }
    }

    async fn run_external(&self, external: &External, bytes: &[u8]) -> Result<Extracted> {
        // Written out because these programs take a path, not a stream. In a
        // temporary directory that is removed when it drops, including on the
        // paths that return early.
        let dir = tempdir()?;
        let path = dir.path().join("paper.pdf");
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|e| Error::internal(format!("could not stage the file: {e}")))?;

        let text = external.read(&path).await?;
        Ok(crate::bound(&crate::normalise(&text)))
    }
}

/// A directory that removes itself, without a dependency for it.
fn tempdir() -> Result<TempDir> {
    let base = std::env::temp_dir().join(format!(
        "yinkote-pdf-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&base)
        .map_err(|e| Error::internal(format!("could not make a working directory: {e}")))?;
    Ok(TempDir(base))
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shell script standing in for a layout model, so the seam is tested
    /// without a gigabyte of weights.
    fn script(body: &str) -> (TempDir, External) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("reader.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let external = External {
            command: path.to_string_lossy().into_owned(),
            args: vec!["{}".into()],
            timeout: Duration::from_secs(10),
        };
        (dir, external)
    }

    /// Enough alphanumerics to pass `is_useful`, which is what tells a real
    /// answer from the few stray glyphs a scan yields.
    fn plenty() -> String {
        "the transformer architecture ".repeat(20)
    }

    #[tokio::test]
    async fn with_nothing_configured_the_builtin_reader_answers() {
        let pipeline = Pipeline::default();
        // Not a PDF, so this is the built-in reader's error and not a report
        // that no external reader is installed.
        assert!(pipeline.read(b"not a pdf").await.is_err());
    }

    #[tokio::test]
    async fn an_external_reader_is_used_when_asked() {
        let (_dir, external) = script(&format!("echo '{}'", plenty()));
        let pipeline = Pipeline { mode: Mode::Always, external: Some(external) };
        let got = pipeline.read(b"not a pdf").await.unwrap();
        assert!(got.text.contains("transformer"));
    }

    /// The state every machine starts in: the command names something that is
    /// not installed. A paper the built-in reader handles must still be read.
    #[tokio::test]
    async fn a_missing_program_does_not_become_the_answer() {
        let pipeline = Pipeline {
            mode: Mode::Always,
            external: Some(External {
                command: "definitely-not-installed-anywhere".into(),
                args: vec!["{}".into()],
                timeout: Duration::from_secs(5),
            }),
        };
        // Falls through to the built-in reader, whose verdict on this input is
        // "not a PDF" — the file's problem, not the configuration's.
        let err = pipeline.read(b"not a pdf").await.unwrap_err().to_string();
        assert!(err.contains("PDF"), "got {err}");
    }

    /// `Fallback` is the setting worth having: a layout model is slow, and the
    /// case it is needed for is the one the built-in reader cannot do at all.
    /// `Fallback` is the setting worth having: a layout model is slow, and the
    /// case it is needed for is the one the built-in reader cannot do at all.
    #[tokio::test]
    async fn fallback_does_not_pay_for_a_paper_that_read_fine() {
        // The script's answer is long enough to be *preferred* if it were
        // consulted. An earlier version echoed one short line, which is not
        // useful text — so the external reader ran, its answer was discarded
        // for being too thin, and the test passed without testing anything.
        let (_dir, external) = script(&format!("echo 'external {}'", plenty()));
        let pipeline = Pipeline { mode: Mode::Fallback, external: Some(external) };

        let real = std::fs::read("tests/data/minimal.pdf").unwrap();
        let got = pipeline.read(&real).await.unwrap();
        assert!(got.is_useful(), "the fixture must be a paper the built-in reader can read");
        assert!(!got.text.contains("external"), "it paid for a reader it did not need");
    }

    /// And the other half: when the built-in reader comes back with nothing —
    /// a scan — the external one is what answers.
    #[tokio::test]
    async fn fallback_is_used_when_there_is_no_text_layer() {
        let (_dir, external) = script(&format!("echo 'external {}'", plenty()));
        let pipeline = Pipeline { mode: Mode::Fallback, external: Some(external) };

        // Not a PDF at all, which is the built-in reader's other failure and
        // reaches the same branch as a page of images.
        let got = pipeline.read(b"not a pdf").await.unwrap();
        assert!(got.text.contains("external"), "nothing else could have read it");
    }

    #[tokio::test]
    async fn a_reader_that_hangs_is_given_up_on() {
        let (_dir, mut external) = script("sleep 30");
        external.timeout = Duration::from_millis(300);
        let started = std::time::Instant::now();
        let pipeline = Pipeline { mode: Mode::Always, external: Some(external) };
        let _ = pipeline.read(b"not a pdf").await;
        assert!(started.elapsed() < Duration::from_secs(5), "it waited for the whole sleep");
    }

    /// Without the placeholder the program is run on no file at all and
    /// reports success on an empty document — a failure with no error.
    #[tokio::test]
    async fn a_command_that_never_names_the_file_is_refused() {
        let external = External {
            command: "echo".into(),
            args: vec!["--output".into()],
            timeout: Duration::from_secs(5),
        };
        assert!(!external.names_the_file());
        let err = external.read(Path::new("/tmp/x.pdf")).await.unwrap_err().to_string();
        assert!(err.contains("{}"), "got {err}");
    }

    #[test]
    fn the_modes_are_read_from_what_somebody_would_write() {
        assert_eq!(Mode::parse("fallback"), Mode::Fallback);
        assert_eq!(Mode::parse("auto"), Mode::Fallback);
        assert_eq!(Mode::parse("Always"), Mode::Always);
        assert_eq!(Mode::parse(""), Mode::Off);
        assert_eq!(Mode::parse("nonsense"), Mode::Off);
    }
}
