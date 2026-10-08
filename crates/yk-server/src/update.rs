//! Installation and self-update share the shipped bootstrap scripts.

use std::path::Path;
use tokio::process::Command;

const POSIX_INSTALLER: &str = include_str!("../../../install.sh");
const WINDOWS_INSTALLER: &str = include_str!("../../../install.ps1");

fn command(executable: &Path, version: &str, windows: bool) -> Command {
    let mut command = if windows {
        let mut command = Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            WINDOWS_INSTALLER,
        ]);
        command
    } else {
        let mut command = Command::new("sh");
        command.args(["-c", POSIX_INSTALLER, "yinkote-update"]);
        command
    };
    command
        .env("YINKOTE_UPDATE_TARGET", executable)
        .env("YINKOTE_CURRENT_VERSION", version)
        .kill_on_drop(true);
    command
}

pub async fn run() -> anyhow::Result<()> {
    if !matches!(std::env::consts::OS, "linux" | "macos" | "windows") {
        anyhow::bail!("updates are not supported on {}", std::env::consts::OS);
    }
    let executable = std::env::current_exe()?;
    let status = command(&executable, env!("CARGO_PKG_VERSION"), cfg!(windows))
        .status()
        .await
        .map_err(|error| anyhow::anyhow!("could not start the installer: {error}"))?;
    if !status.success() {
        anyhow::bail!("update failed ({status}); see the installer error above");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_executable_path_is_data_not_interpolated_shell_source() {
        let path = Path::new("/a path/with 'quotes'/$characters/yinkote");
        for windows in [false, true] {
            let command = command(path, "1.2.3", windows);
            let command = command.as_std();
            assert!(command.get_envs().any(|(name, value)| {
                name == "YINKOTE_UPDATE_TARGET" && value == Some(path.as_os_str())
            }));
            assert!(!command
                .get_args()
                .any(|argument| argument == path.as_os_str()));
            assert!(command.get_args().any(|argument| {
                argument
                    == if windows {
                        WINDOWS_INSTALLER
                    } else {
                        POSIX_INSTALLER
                    }
            }));
        }
    }
}
