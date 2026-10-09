//! Serializable user settings, validated before applying them to live views.
use plugin_runtime::plugin_protocol::ui::SideTabsPosition;
use rust_i18n::t;
use serde::{Deserialize, Serialize};

/// Named shell program and separately escaped arguments; never interpolated as a command.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// Optional palette entries inherit the editor theme. ANSI colors always contain 16 entries.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Palette {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub cursor: Option<String>,
    pub selection: Option<String>,
    pub ansi: Option<[String; 16]>,
}

/// User-level terminal settings; commands in projects are never executed on startup.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    /// Sidebar placement also moves the canvas origin; older settings default to the right.
    pub tab_position: SideTabsPosition,
    pub font_family: String,
    pub font_size: f32,
    pub history: usize,
    pub default_profile: usize,
    pub profiles: Vec<Profile>,
    pub theme: Palette,
    /// Optional command for the explicit Run Project button; never executed at startup.
    pub run_command: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let profiles = default_profiles(cfg!(windows));
        Self {
            enabled: true,
            tab_position: SideTabsPosition::Right,
            font_family: "Cascadia Mono".into(),
            font_size: 14.,
            history: 10_000,
            default_profile: 0,
            profiles,
            theme: Palette::default(),
            run_command: None,
        }
    }
}

/// Choose platform defaults from the host environment, never the build target.
fn default_profiles(windows: bool) -> Vec<Profile> {
    if windows {
        vec![
            Profile {
                name: "PowerShell".into(),
                program: "powershell.exe".into(),
                args: vec!["-NoLogo".into()],
            },
            Profile {
                name: "Command Prompt".into(),
                program: "cmd.exe".into(),
                args: vec![],
            },
            Profile {
                name: "PowerShell 7".into(),
                program: "pwsh.exe".into(),
                args: vec!["-NoLogo".into()],
            },
            Profile {
                name: "WSL".into(),
                program: "wsl.exe".into(),
                args: vec![],
            },
        ]
    } else {
        vec![Profile {
            name: "Shell".into(),
            program: "/bin/sh".into(),
            args: vec![],
        }]
    }
}

impl Settings {
    /// Build startup settings for the OS on which the native process service will execute.
    pub fn for_os(os: &str) -> Self {
        Self {
            profiles: default_profiles(os == "windows"),
            ..Self::default()
        }
    }
    /// Fill omitted profiles before validation; explicit user programs are never replaced.
    pub fn parse_for_os(source: &str, os: &str) -> anyhow::Result<Self> {
        let mut value: serde_json::Value = serde_json::from_str(source)?;
        if let Some(object) = value.as_object_mut() {
            if !object.contains_key("profiles") {
                object.insert(
                    "profiles".into(),
                    serde_json::to_value(default_profiles(os == "windows"))?,
                );
            }
        }
        Self::parse(&serde_json::to_string(&value)?)
    }
    /// Reject malformed values instead of crashing or silently selecting another shell.
    pub fn parse(source: &str) -> anyhow::Result<Self> {
        let settings: Self = serde_json::from_str(source)?;
        anyhow::ensure!(
            !settings.profiles.is_empty()
                && settings.profiles.len() <= 32
                && settings.font_family.len() <= 256,
            t!("terminal.invalid_profiles")
        );
        anyhow::ensure!(
            (8. ..=32.).contains(&settings.font_size),
            t!("terminal.invalid_font_size")
        );
        anyhow::ensure!(
            settings.default_profile < settings.profiles.len(),
            t!("terminal.invalid_default_profile")
        );
        anyhow::ensure!(settings.history <= 100_000, t!("terminal.invalid_history"));
        anyhow::ensure!(
            !settings.font_family.trim().is_empty(),
            t!("terminal.invalid_font_family")
        );
        for profile in &settings.profiles {
            anyhow::ensure!(
                profile.name.len() <= 128
                    && profile.program.len() <= 4096
                    && !profile.program.contains('\0')
                    && profile.args.len() <= 128
                    && profile
                        .args
                        .iter()
                        .all(|arg| arg.len() <= 32768 && !arg.contains('\0')),
                t!("terminal.invalid_profile")
            );
            anyhow::ensure!(
                !profile.name.trim().is_empty() && !profile.program.trim().is_empty(),
                t!("terminal.empty_profile")
            );
        }
        let p = &settings.theme;
        for color in [&p.background, &p.foreground, &p.cursor, &p.selection]
            .into_iter()
            .flatten()
            .chain(p.ansi.iter().flatten())
        {
            anyhow::ensure!(
                color.len() == 7
                    && color.starts_with('#')
                    && color[1..].bytes().all(|b| b.is_ascii_hexdigit()),
                t!("terminal.invalid_color", color = color)
            );
        }
        Ok(settings)
    }
}
