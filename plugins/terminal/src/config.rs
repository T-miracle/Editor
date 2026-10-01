//! Serializable user settings, validated before applying them to live views.
use plugin_protocol::ui::SideTabsPosition;
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
        #[cfg(windows)]
        let profiles = vec![
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
        ];
        #[cfg(not(windows))]
        let profiles = vec![Profile {
            name: "Shell".into(),
            program: std::env::var("SHELL").unwrap_or("/bin/sh".into()),
            args: vec![],
        }];
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

impl Settings {
    /// Reject malformed values instead of crashing or silently selecting another shell.
    pub fn parse(source: &str) -> anyhow::Result<Self> {
        let settings: Self = serde_json::from_str(source)?;
        anyhow::ensure!(
            (8. ..=32.).contains(&settings.font_size),
            "font_size must be between 8 and 32"
        );
        anyhow::ensure!(
            settings.default_profile < settings.profiles.len(),
            "default_profile does not identify a shell"
        );
        anyhow::ensure!(
            settings.history <= 100_000,
            "history must be at most 100000"
        );
        anyhow::ensure!(
            !settings.font_family.trim().is_empty(),
            "font_family is empty"
        );
        for profile in &settings.profiles {
            anyhow::ensure!(
                !profile.name.trim().is_empty() && !profile.program.trim().is_empty(),
                "Shell name and program must not be empty"
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
                "Invalid color {color}; use #RRGGBB"
            );
        }
        Ok(settings)
    }
}
