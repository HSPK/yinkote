//! Starting Yinkote when the machine starts.
//!
//! **Why this exists.** The premise is a background service the user starts
//! once and forgets. "Starts once" only holds if it survives a reboot, and the
//! way to arrange that differs on every platform and is not something anybody
//! should have to look up. `yinkote service install` writes the right file in
//! the right place and says what it did.
//!
//! **What it deliberately does not do.** It installs a *user* service, never a
//! system one. A system service would need root, would run as somebody else,
//! and would put a personal library in a place the person cannot reach. A
//! reference manager is not infrastructure.
//!
//! **The shape.** Where the file goes and what is in it are pure functions,
//! because that is the part worth testing and the part that is wrong on the
//! platform the author does not own. Only [`install`] and [`uninstall`] touch
//! the disk, and neither pretends to have done more than it did: on Linux the
//! unit still has to be enabled, and saying so is better than a message that
//! reads like success.

use std::path::{Path, PathBuf};

/// The platforms with a native way to start a program at login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// systemd user units.
    Linux,
    /// launchd agents.
    MacOs,
    /// A script in the Startup folder.
    ///
    /// Not the registry: it needs no crate, it is visible to the user in a
    /// folder they can open, and removing it is deleting a file. A registry
    /// entry is none of those things.
    Windows,
}

impl Platform {
    /// What this binary was built for.
    pub fn current() -> Option<Self> {
        match std::env::consts::OS {
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::MacOs),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }

    /// The command that finishes the job, when one is needed.
    ///
    /// systemd will not notice a new unit until it is told to look, and will
    /// not start it until it is enabled. Writing the file and reporting success
    /// would leave somebody with a service that never runs.
    pub fn activation(self) -> Option<&'static str> {
        match self {
            Self::Linux => Some("systemctl --user daemon-reload && systemctl --user enable --now yinkote"),
            // launchd and the Startup folder both take effect on next login.
            Self::MacOs | Self::Windows => None,
        }
    }
}

/// Where the autostart file belongs, given a home directory.
///
/// Takes `home` rather than reading it, so every platform's answer can be
/// checked on any platform — which is the only way the two the author is not
/// using are ever going to be right.
pub fn unit_path(platform: Platform, home: &Path) -> PathBuf {
    match platform {
        Platform::Linux => home.join(".config/systemd/user/yinkote.service"),
        Platform::MacOs => home.join("Library/LaunchAgents/com.yinkote.server.plist"),
        Platform::Windows => home
            .join("AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup/yinkote.cmd"),
    }
}

/// The autostart file's contents.
///
/// `exe` is written absolute: a login session's `PATH` is not a shell's, and a
/// bare program name is the commonest reason one of these silently does
/// nothing.
pub fn unit_text(platform: Platform, exe: &Path, data_dir: &Path, port: u16) -> String {
    let exe = exe.display();
    let data = data_dir.display();
    match platform {
        Platform::Linux => format!(
            "[Unit]\n\
             Description=Yinkote reference manager\n\
             Documentation=https://github.com/yinkote/yinkote\n\
             After=network.target\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart={exe} --data-dir {data} --port {port}\n\
             Restart=on-failure\n\
             RestartSec=5\n\
             \n\
             [Install]\n\
             WantedBy=default.target\n"
        ),
        Platform::MacOs => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
             \t<key>Label</key><string>com.yinkote.server</string>\n\
             \t<key>ProgramArguments</key>\n\
             \t<array>\n\
             \t\t<string>{exe}</string>\n\
             \t\t<string>--data-dir</string><string>{data}</string>\n\
             \t\t<string>--port</string><string>{port}</string>\n\
             \t</array>\n\
             \t<key>RunAtLoad</key><true/>\n\
             \t<key>KeepAlive</key><true/>\n\
             </dict>\n\
             </plist>\n"
        ),
        // `start ""` so the console window closes immediately and the shell
        // does not wait for the server to exit; the empty title is required or
        // the first quoted argument is taken as one.
        Platform::Windows => format!(
            "@echo off\r\n\
             start \"\" /b \"{exe}\" --data-dir \"{data}\" --port {port}\r\n"
        ),
    }
}

/// What an install did, so the caller can report it rather than guess.
pub struct Installed {
    pub path: PathBuf,
    pub activation: Option<&'static str>,
}

/// Write the autostart file for this platform.
pub fn install(data_dir: &Path, port: u16) -> std::io::Result<Installed> {
    let platform = Platform::current().ok_or_else(unsupported)?;
    let home = home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no home directory to install into")
    })?;
    // The path this binary is at, not the name it was invoked by.
    let exe = std::env::current_exe()?;

    let path = unit_path(platform, &home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, unit_text(platform, &exe, data_dir, port))?;

    Ok(Installed { path, activation: platform.activation() })
}

/// Remove it, reporting whether there was anything to remove.
pub fn uninstall() -> std::io::Result<Option<PathBuf>> {
    let platform = Platform::current().ok_or_else(unsupported)?;
    let home = home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no home directory")
    })?;
    let path = unit_path(platform, &home);
    if !path.exists() {
        return Ok(None);
    }
    std::fs::remove_file(&path)?;
    Ok(Some(path))
}

/// Whether an autostart file is in place, and where.
pub fn status() -> Option<PathBuf> {
    let platform = Platform::current()?;
    let path = unit_path(platform, &home_dir()?);
    path.exists().then_some(path)
}

fn unsupported() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        format!("no autostart mechanism known for {}", std::env::consts::OS),
    )
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Platform; 3] = [Platform::Linux, Platform::MacOs, Platform::Windows];

    fn text(platform: Platform) -> String {
        unit_text(platform, Path::new("/opt/yinkote/yinkote"), Path::new("/home/a/.yinkote"), 23119)
    }

    #[test]
    fn every_platform_writes_under_the_users_own_home() {
        // Never a system service: it would need root, run as somebody else,
        // and put a personal library where the person cannot reach it.
        for platform in ALL {
            let path = unit_path(platform, Path::new("/home/ada"));
            assert!(
                path.starts_with("/home/ada"),
                "{platform:?} would install to {}",
                path.display()
            );
        }
    }

    #[test]
    fn every_platform_names_the_binary_absolutely() {
        // A login session's PATH is not a shell's. A bare program name is the
        // commonest reason one of these silently does nothing.
        for platform in ALL {
            assert!(
                text(platform).contains("/opt/yinkote/yinkote"),
                "{platform:?} did not write the full path"
            );
        }
    }

    #[test]
    fn every_platform_passes_the_data_directory_and_port() {
        // Without these it starts with the defaults, which is a different
        // library on a different port from the one being installed.
        for platform in ALL {
            let out = text(platform);
            assert!(out.contains("/home/a/.yinkote"), "{platform:?} lost the data directory");
            assert!(out.contains("23119"), "{platform:?} lost the port");
        }
    }

    #[test]
    fn the_systemd_unit_restarts_but_does_not_spin() {
        let out = text(Platform::Linux);
        assert!(out.contains("Restart=on-failure"));
        // Without a delay a program that fails immediately is restarted as
        // fast as the machine can manage.
        assert!(out.contains("RestartSec="));
        assert!(out.contains("WantedBy=default.target"), "or it is never started");
    }

    #[test]
    fn the_launch_agent_asks_to_be_run_at_login() {
        let out = text(Platform::MacOs);
        assert!(out.contains("<key>RunAtLoad</key><true/>"));
        assert!(out.contains("com.yinkote.server"));
        // Arguments go in the array one element at a time; a single string
        // would be treated as one program name containing spaces.
        assert!(out.contains("<string>--data-dir</string>"));
    }

    #[test]
    fn the_startup_script_does_not_hold_a_console_open() {
        let out = text(Platform::Windows);
        // The empty title is required: without it `start` takes the first
        // quoted argument as the window title and never runs the program.
        assert!(out.contains("start \"\" /b"), "{out}");
        assert!(out.contains("\r\n"), "a .cmd file wants CRLF");
    }

    #[test]
    fn only_linux_needs_a_further_command() {
        // systemd will not notice a new unit until told to look. launchd and
        // the Startup folder take effect at the next login, so claiming a
        // command is needed would be noise.
        assert!(Platform::Linux.activation().is_some());
        assert!(Platform::MacOs.activation().is_none());
        assert!(Platform::Windows.activation().is_none());
    }

    #[test]
    fn a_path_with_spaces_survives_on_every_platform() {
        // "Program Files", and every macOS user who named their disk.
        let out = unit_text(
            Platform::Windows,
            Path::new("C:/Program Files/Yinkote/yinkote.exe"),
            Path::new("C:/Users/Ada/Yinkote Library"),
            23119,
        );
        assert!(out.contains("\"C:/Program Files/Yinkote/yinkote.exe\""));
        assert!(out.contains("\"C:/Users/Ada/Yinkote Library\""));
    }
}
