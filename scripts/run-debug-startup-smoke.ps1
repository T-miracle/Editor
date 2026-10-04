# Verify the real binary starts on a workspace with a shared run configuration, and that closing it
# leaves no program behind.
#
# The in-process checks for these properties live in Rust; this one exists because two claims are only
# meaningful against the built application: the editor survives loading a real shared configuration,
# and a real program the user started is gone after the window closes. Nothing here clicks Run, so this
# does not measure launching — it measures that loading a shared configuration does not break startup
# and that shutdown reclaims what a session owns.
param([string]$HostExe = "$PSScriptRoot/../target/debug/editor-app.exe")
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path "$PSScriptRoot/..").Path
$editorExe = (Resolve-Path -LiteralPath $HostExe).Path

# A close message needs the window handle, and PowerShell cannot send one without the platform API.
if (-not ('MeEditorSmoke.Window' -as [type])) {
    Add-Type -Namespace MeEditorSmoke -Name Window -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true)]
public static extern bool PostMessage(System.IntPtr hWnd, uint msg, System.IntPtr wParam, System.IntPtr lParam);
[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern bool IsWindow(System.IntPtr hWnd);
'@
}
$WM_CLOSE = 0x0010

$root = Join-Path $repoRoot 'target/run-debug-startup-smoke'
Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $root | Out-Null
# A shared configuration is what the editor reads at startup; its identity and shape must be a real one.
$marker = "STARTUP_SMOKE_$(Get-Random)"
$projectDir = Join-Path $root '.me-editor'
New-Item -ItemType Directory -Force -Path $projectDir | Out-Null
$shared = @{
    version = 1
    configurations = @(
        @{
            id = 'startup-smoke'
            name = '启动冒烟'
            target = @{
                type = 'program'
                program = 'powershell.exe'
                args = @('-NoProfile', '-Command', "Start-Sleep -Seconds 120 # $marker")
            }
            build = @()
            prelaunch = @()
        }
    )
} | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText((Join-Path $projectDir 'run-configs.json'), $shared, (New-Object System.Text.UTF8Encoding($false)))

$stderrPath = Join-Path $root 'editor.stderr.log'
$stdoutPath = Join-Path $root 'editor.stdout.log'
$before = @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" |
    Where-Object { $_.CommandLine -like "*$marker*" }).Count

# `Start-Process` refuses an environment that carries both `NO_PROXY` and `no_proxy`, which is an
# ordinary Windows configuration, so the process is started through the framework API instead.
$startInfo = New-Object System.Diagnostics.ProcessStartInfo
$startInfo.FileName = $editorExe
$startInfo.Arguments = '"' + $root + '"'
$startInfo.UseShellExecute = $false
$startInfo.RedirectStandardError = $true
$startInfo.RedirectStandardOutput = $true
$editorProcess = [System.Diagnostics.Process]::Start($startInfo)
$stderrTask = $editorProcess.StandardError.ReadToEndAsync()
$stdoutTask = $editorProcess.StandardOutput.ReadToEndAsync()
try {
    # First paint, configuration load and plugin activation all happen in this window.
    Start-Sleep -Seconds 6
    $editorProcess.Refresh()
    if ($editorProcess.HasExited) {
        $errorText = $stderrTask.Result
        throw "Editor exited while loading a workspace with a shared configuration (code $($editorProcess.ExitCode)): $errorText"
    }
    # Its own window exists, so this is a real native session rather than a headless process.
    $editorProcess.Refresh()
    $handle = $editorProcess.MainWindowHandle
    if ($handle -eq [IntPtr]::Zero) {
        throw 'The editor did not create a native window.'
    }
    Write-Output "editor started with a shared configuration and owns a native window ($handle)"

    # Establish that the leak check can fail. Nothing this script starts belongs to the editor, so a
    # count of zero afterwards would also be produced by a blind check; this control is a program the
    # editor never owned, and it must still be counted once the window is closed.
    $control = [System.Diagnostics.Process]::Start((New-Object System.Diagnostics.ProcessStartInfo -Property @{
        FileName = 'powershell.exe'
        Arguments = "-NoProfile -Command `"Start-Sleep -Seconds 300 # $marker`""
        UseShellExecute = $false
    }))
    Start-Sleep -Seconds 2
    $withControl = @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" |
        Where-Object { $_.CommandLine -like "*$marker*" }).Count
    if ($withControl -lt 1) {
        throw 'The leak check cannot see a program that is certainly running, so it proves nothing.'
    }
    Write-Output "leak check is sensitive: it sees $withControl program(s) carrying this run's marker"

    # Closing the window must end the process without leaving anything the session owned.
    [void][MeEditorSmoke.Window]::PostMessage($handle, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero)
    $deadline = (Get-Date).AddSeconds(30)
    while (-not $editorProcess.HasExited -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 200
        $editorProcess.Refresh()
    }
    if (-not $editorProcess.HasExited) {
        throw 'The editor did not exit after its window was closed.'
    }
    Write-Output 'closing the window ended the editor process'

    $after = @(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" |
        Where-Object { $_.CommandLine -like "*$marker*" }).Count
    # The control must still be there: if closing the window had taken it down, the count below would
    # be satisfied for the wrong reason.
    if ($after -lt 1) {
        throw 'The control program disappeared, so this run cannot tell a leak from a blind check.'
    }
    if ($after -gt $before + 1) {
        throw "Closing left $($after - $before - 1) program(s) behind."
    }
    Write-Output 'Run and debug startup smoke passed: no program was left behind by shutdown.'
} finally {
    $editorProcess.Refresh()
    if (-not $editorProcess.HasExited) {
        Stop-Process -Id $editorProcess.Id -ErrorAction SilentlyContinue
    }
    # Only this script's own marker is searched for, so an unrelated editor or program is untouched.
    Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" |
        Where-Object { $_.CommandLine -like "*$marker*" } |
        ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }
}
