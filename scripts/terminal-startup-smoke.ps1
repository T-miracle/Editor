# Verify the native window survives its first dock layout and paint on Windows.
$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$editorExe = Join-Path $repoRoot 'target/debug/editor-app.exe'
if (-not (Test-Path -LiteralPath $editorExe)) {
    throw 'Build the editor first: cargo build -p editor-app'
}
$workspace = Join-Path $repoRoot 'target/terminal-startup-smoke'
New-Item -ItemType Directory -Force -Path $workspace | Out-Null
$stderrPath = Join-Path $repoRoot 'target/terminal-startup-smoke.stderr.log'
$stdoutPath = Join-Path $repoRoot 'target/terminal-startup-smoke.stdout.log'
$editorProcess = Start-Process -FilePath $editorExe -ArgumentList @($workspace) -WindowStyle Hidden -PassThru -RedirectStandardError $stderrPath -RedirectStandardOutput $stdoutPath
try {
    Start-Sleep -Seconds 4
    $editorProcess.Refresh()
    if ($editorProcess.HasExited) {
        $errorText = Get-Content -LiteralPath $stderrPath -Raw -ErrorAction SilentlyContinue
        throw "Editor exited during first paint with code $($editorProcess.ExitCode): $errorText"
    }
    Write-Output 'Terminal startup smoke test passed: editor stayed alive after first paint.'
} finally {
    # Stop only the process created by this script, leaving user editor windows intact.
    $editorProcess.Refresh()
    if (-not $editorProcess.HasExited) {
        Stop-Process -Id $editorProcess.Id -ErrorAction SilentlyContinue
    }
}
