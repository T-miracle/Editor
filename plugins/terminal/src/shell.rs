//! Optional shell integration remains plugin code, including current-directory tracking.
use super::Profile;
use base64::Engine;

/// Preserve a user's PowerShell prompt and emit private, nonprinting cwd metadata before it.
pub(super) fn arguments(profile: &Profile) -> Vec<String> {
    let mut args = profile.args.clone();
    let tool = profile
        .program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        tool.as_str(),
        "powershell.exe" | "powershell" | "pwsh.exe" | "pwsh"
    ) && !args.iter().any(|a| {
        matches!(
            a.to_ascii_lowercase().as_str(),
            "-command" | "-c" | "-file" | "-f" | "-encodedcommand"
        )
    }) {
        args.extend(["-NoExit".into(),"-Command".into(),r#"$global:__MeEditorPrompt=$function:prompt; function global:prompt { $p=[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes((Get-Location).ProviderPath)); [Console]::Write(([char]27).ToString()+']633;P;Cwd64='+$p+[char]7); & $global:__MeEditorPrompt }"#.into()]);
    }
    args
}
#[derive(Default)]
pub(super) struct Metadata {
    pub cwd: Option<String>,
}
impl vte::Perform for Metadata {
    /// Accept only bounded cwd metadata; VT screen behavior stays with the upstream parser.
    fn osc_dispatch(&mut self, params: &[&[u8]], _: bool) {
        if params.len() == 3 && params[0] == b"633" && params[1] == b"P" {
            if let Some(encoded) = params[2].strip_prefix(b"Cwd64=") {
                if encoded.len() <= 16384 {
                    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                        if let Ok(path) = String::from_utf8(bytes) {
                            if !path.is_empty() && !path.contains('\0') {
                                self.cwd = Some(path);
                            }
                        }
                    }
                }
            }
        }
    }
}
