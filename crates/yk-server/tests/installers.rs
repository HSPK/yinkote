#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sha2::{Digest, Sha256};

const INSTALLER: &str = include_str!("../../../install.sh");

struct Fixture {
    root: tempfile::TempDir,
    tools: PathBuf,
    destination: PathBuf,
}

fn executable(path: &Path, source: &str) {
    fs::write(path, source).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let tools = root.path().join("tools");
        fs::create_dir(&tools).unwrap();
        executable(&tools.join("uname"), "#!/bin/sh\ncase \"$1\" in -s) echo \"${FAKE_OS:-Linux}\";; -m) echo \"${FAKE_ARCH:-x86_64}\";; esac\n");
        executable(
            &tools.join("ldd"),
            "#!/bin/sh\necho \"${FAKE_LIBC:-glibc}\"\n",
        );
        executable(
            &tools.join("sysctl"),
            "#!/bin/sh\necho \"${FAKE_ROSETTA:-0}\"\n",
        );
        executable(
            &tools.join("curl"),
            r#"#!/bin/sh
set -eu
out=
head=no
url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output) out=$2; shift 2 ;;
        --write-out|--proto|--proto-redir|--retry|--connect-timeout|--max-time) shift 2 ;;
        --head) head=yes; shift ;;
        --*) shift ;;
        *) url=$1; shift ;;
    esac
done
printf '%s\n' "$url" >> "$FIXTURE_ROOT/requests"
if [ "${FAKE_DOWNLOAD_FAILURE:-no}" = yes ] && [ "$head" = no ]; then exit 22; fi
if [ "$head" = yes ]; then
    printf '%s' "https://github.com/HSPK/yinkote/releases/tag/${FAKE_TAG:-v0.1.2}"
else
    case "$url" in
        *.sha256)
            if [ "${FAKE_CHECKSUM_FAILURE:-no}" = yes ]; then
                printf '%064d  binary\n' 0 > "$out"
            else
                cat "$FIXTURE_ROOT/digest" > "$out"
            fi
            ;;
        *) cat "$FIXTURE_ROOT/binary" > "$out" ;;
    esac
fi
"#,
        );
        let binary = "#!/bin/sh\nprintf 'yinkote 0.1.2\\n'\n";
        fs::write(root.path().join("binary"), binary).unwrap();
        fs::write(
            root.path().join("digest"),
            format!("{:x}  binary\n", Sha256::digest(binary)),
        )
        .unwrap();
        let destination = root
            .path()
            .join("directory with 'quotes' $literal")
            .join("yinkote");
        Self {
            root,
            tools,
            destination,
        }
    }

    fn environment(&self, command: &mut Command) {
        let mut paths = vec![self.tools.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        command
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("HOME", self.root.path())
            .env("FIXTURE_ROOT", self.root.path())
            .env("YINKOTE_INSTALL_DIR", self.destination.parent().unwrap())
            .env_remove("YINKOTE_UPDATE_TARGET")
            .env_remove("YINKOTE_CURRENT_VERSION");
    }

    fn run(&self, extra: &[(&str, &str)]) -> Output {
        let mut command = Command::new("sh");
        self.environment(&mut command);
        command
            .envs(extra.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(INSTALLER.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn existing(&self) {
        fs::create_dir_all(self.destination.parent().unwrap()).unwrap();
        executable(&self.destination, "#!/bin/sh\necho old\n");
    }

    fn assert_clean(&self) {
        if let Ok(entries) = fs::read_dir(self.destination.parent().unwrap()) {
            assert!(entries
                .map(|e| e.unwrap().file_name())
                .all(|name| name == "yinkote"));
        }
    }
}

#[test]
fn piped_install_selects_every_published_unix_target() {
    for (os, arch, target) in [
        ("Linux", "x86_64", "x86_64-unknown-linux-gnu"),
        ("Linux", "aarch64", "aarch64-unknown-linux-gnu"),
        ("Darwin", "x86_64", "x86_64-apple-darwin"),
        ("Darwin", "arm64", "aarch64-apple-darwin"),
    ] {
        let fixture = Fixture::new();
        let result = fixture.run(&[("FAKE_OS", os), ("FAKE_ARCH", arch)]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            fs::read(&fixture.destination).unwrap(),
            fs::read(fixture.root.path().join("binary")).unwrap()
        );
        let requests = fs::read_to_string(fixture.root.path().join("requests")).unwrap();
        assert!(
            requests.contains(&format!("/download/v0.1.2/yinkote-{target}\n")),
            "{requests}"
        );
        assert!(
            requests.contains(&format!("/download/v0.1.2/yinkote-{target}.sha256\n")),
            "{requests}"
        );
        fixture.assert_clean();
    }
}

#[test]
fn rosetta_selects_the_native_apple_silicon_binary() {
    let fixture = Fixture::new();
    assert!(fixture
        .run(&[
            ("FAKE_OS", "Darwin"),
            ("FAKE_ARCH", "x86_64"),
            ("FAKE_ROSETTA", "1")
        ])
        .status
        .success());
    assert!(fs::read_to_string(fixture.root.path().join("requests"))
        .unwrap()
        .contains("aarch64-apple-darwin"));
}

#[test]
fn the_default_installation_stays_in_the_users_home() {
    let fixture = Fixture::new();
    let mut command = Command::new("sh");
    fixture.environment(&mut command);
    let result = command
        .env_remove("YINKOTE_INSTALL_DIR")
        .args(["-c", INSTALLER, "installer-test"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(fixture.root.path().join(".local/bin/yinkote").exists());
    assert!(!fixture.destination.exists());
}

#[test]
fn failures_never_replace_an_existing_executable() {
    for extra in [
        vec![("FAKE_CHECKSUM_FAILURE", "yes")],
        vec![("FAKE_DOWNLOAD_FAILURE", "yes")],
        vec![("FAKE_ARCH", "riscv64")],
        vec![("FAKE_LIBC", "musl libc")],
        vec![("FAKE_TAG", "not-a-version")],
    ] {
        let fixture = Fixture::new();
        fixture.existing();
        let before = fs::read(&fixture.destination).unwrap();
        let result = fixture.run(&extra);
        assert!(!result.status.success());
        assert_eq!(fs::read(&fixture.destination).unwrap(), before);
        fixture.assert_clean();
    }
}

#[test]
fn an_incompatible_but_checksum_valid_binary_is_not_installed() {
    let fixture = Fixture::new();
    fixture.existing();
    let binary = "#!/bin/sh\nexit 1\n";
    fs::write(fixture.root.path().join("binary"), binary).unwrap();
    fs::write(
        fixture.root.path().join("digest"),
        format!("{:x}\n", Sha256::digest(binary)),
    )
    .unwrap();
    assert!(!fixture.run(&[]).status.success());
    assert!(fs::read_to_string(&fixture.destination)
        .unwrap()
        .contains("echo old"));
    fixture.assert_clean();
}

#[test]
fn updating_skips_equal_or_newer_versions_without_downloading_assets() {
    for current in [
        "0.1.2",
        "0.1.3",
        "0.2.0",
        "1.0.0",
        "0.1.2+build-with-hyphens",
    ] {
        let fixture = Fixture::new();
        fixture.existing();
        let result = fixture.run(&[
            (
                "YINKOTE_UPDATE_TARGET",
                fixture.destination.to_str().unwrap(),
            ),
            ("YINKOTE_CURRENT_VERSION", current),
        ]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            fs::read_to_string(fixture.root.path().join("requests"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!(fs::read_to_string(&fixture.destination)
            .unwrap()
            .contains("echo old"));
    }
}

#[test]
fn a_prerelease_can_update_to_the_same_stable_version() {
    let fixture = Fixture::new();
    fixture.existing();
    let result = fixture.run(&[
        (
            "YINKOTE_UPDATE_TARGET",
            fixture.destination.to_str().unwrap(),
        ),
        ("YINKOTE_CURRENT_VERSION", "0.1.2-rc.1"),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(fs::read_to_string(&fixture.destination)
        .unwrap()
        .contains("yinkote 0.1.2"));
}

#[test]
fn cli_update_replaces_its_own_path_without_opening_a_library() {
    let fixture = Fixture::new();
    let core = env!("CARGO_PKG_VERSION").split(['-', '+']).next().unwrap();
    let parts: Vec<_> = core.split('.').collect();
    let next = format!(
        "{}.{}.{}",
        parts[0],
        parts[1],
        parts[2].parse::<u64>().unwrap() + 1
    );
    let binary = format!("#!/bin/sh\nprintf 'yinkote {next}\\n'\n");
    fs::write(fixture.root.path().join("binary"), &binary).unwrap();
    fs::write(
        fixture.root.path().join("digest"),
        format!("{:x}\n", Sha256::digest(&binary)),
    )
    .unwrap();
    fs::create_dir_all(fixture.destination.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_yinkote"), &fixture.destination).unwrap();
    let library = fixture.root.path().join("must-not-be-created");
    let mut command = Command::new(&fixture.destination);
    fixture.environment(&mut command);
    command.env("FAKE_TAG", format!("v{next}"));
    let result = command
        .args(["update", "--data-dir"])
        .arg(&library)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!library.exists());
    assert_eq!(
        fs::read(&fixture.destination).unwrap(),
        fs::read(fixture.root.path().join("binary")).unwrap()
    );
    fixture.assert_clean();
}

#[test]
fn cli_rejects_conflicting_commands_before_running_the_installer() {
    let fixture = Fixture::new();
    let mut command = Command::new(env!("CARGO_BIN_EXE_yinkote"));
    fixture.environment(&mut command);
    let output = command.args(["update", "open"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot be combined"));
    assert!(!fixture.root.path().join("requests").exists());
}
