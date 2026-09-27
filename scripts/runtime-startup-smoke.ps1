# Exercise packaged plugins in an actual native window using isolated, disposable test state.
$ErrorActionPreference = 'Stop'
# Hidden windows have no Process.MainWindowHandle; target only this fixture's top-level window.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class PluginSmokeWindow {
    private delegate bool Visitor(IntPtr window, IntPtr state);
    [DllImport("user32.dll")] private static extern bool EnumWindows(Visitor visit, IntPtr state);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassNameW(IntPtr window, StringBuilder name, int capacity);
    [DllImport("user32.dll")] private static extern bool PostMessageW(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    // Enumerate solely to locate the test process; no unrelated window is changed.
    public static bool Close(uint processId) {
        bool sent = false;
        EnumWindows((window, state) => {
            GetWindowThreadProcessId(window, out uint owner);
            var name = new StringBuilder(128);
            GetClassNameW(window, name, name.Capacity);
            if (owner == processId && name.ToString() == "Zed::Window") { sent = PostMessageW(window, 0x0010, IntPtr.Zero, IntPtr.Zero); return false; }
            return true;
        }, IntPtr.Zero);
        return sent;
    }
}
'@
$projectRoot = (Resolve-Path "$PSScriptRoot/..").Path
$runRoot = Join-Path $projectRoot "target/runtime-smoke-$([Guid]::NewGuid().ToString('N'))"
$pluginRoot = Join-Path $runRoot 'plugins'
$workspace = Join-Path $runRoot 'workspace'
New-Item -ItemType Directory -Force $workspace | Out-Null
Push-Location $projectRoot
try {
    cargo run -p plugin-runtime --example prepare_fixture -- $pluginRoot $workspace
    if ($LASTEXITCODE -ne 0) { throw 'Plugin fixture installation failed' }
    $previousPluginHome = $env:ME_EDITOR_PLUGIN_HOME
    $env:ME_EDITOR_PLUGIN_HOME = $pluginRoot
    try {
        $process = Start-Process -FilePath "$projectRoot/target/debug/editor-app.exe" -ArgumentList @($workspace) -WindowStyle Hidden -PassThru -RedirectStandardError "$runRoot/stderr.log" -RedirectStandardOutput "$runRoot/stdout.log"
    } finally { $env:ME_EDITOR_PLUGIN_HOME = $previousPluginHome }
    try {
        Start-Sleep -Seconds 8
        $process.Refresh()
        if ($process.HasExited) { throw "Editor failed during runtime plugin paint: $([IO.File]::ReadAllText("$runRoot/stderr.log"))" }
        # Close normally first so the plugin worker can persist its last snapshot.
        if (-not [PluginSmokeWindow]::Close($process.Id)) { throw 'Could not send close to the hidden test window' }
        if (-not $process.WaitForExit(10000)) { throw 'Editor did not finish normal shutdown' }
        $stateFiles = Get-ChildItem -LiteralPath "$pluginRoot/data/me.terminal" -Filter 'state-*.json'
        if ($stateFiles.Count -eq 0) { throw 'Terminal state was not persisted' }
        Write-Output "PASS: native startup, three dynamic panels and normal shutdown; logs: $runRoot"
    } finally {
        $process.Refresh()
        if (-not $process.HasExited) { Stop-Process -Id $process.Id }
    }
} finally { Pop-Location }
