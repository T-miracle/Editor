# Exercise the SVG component and native GPU painting in an isolated editor window.
param([string]$HostExe = "$PSScriptRoot/../target/debug/editor-app.exe")
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path "$PSScriptRoot/..").Path
$hostPath = (Resolve-Path -LiteralPath $HostExe).Path
$runRoot = Join-Path $projectRoot "target/svg-smoke-$([Guid]::NewGuid().ToString('N'))"
$pluginRoot = Join-Path $runRoot 'plugins'
$workspace = Join-Path $runRoot 'workspace'
New-Item -ItemType Directory -Force $workspace | Out-Null
$sample = Join-Path $workspace 'gear.svg'
# The hole and outer margins are transparent, matching the requested checkerboard composition.
Copy-Item -LiteralPath "$projectRoot/plugins/svg/examples/gear.svg" -Destination $sample
Add-Type -AssemblyName System.Drawing
# PowerShell 7 keeps the drawing implementation separate from its namespace-forwarding assembly.
$drawingReferences = @([System.Drawing.Bitmap].Assembly.Location, [System.Drawing.Color].Assembly.Location)
$drawingReferences += [System.Drawing.Bitmap].GetInterfaces().Assembly.Location
$drawingReferences = $drawingReferences | Sort-Object -Unique
Add-Type -ReferencedAssemblies $drawingReferences -TypeDefinition @'
using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;
public static class SvgSmokeWindow {
    private delegate bool Visitor(IntPtr window, IntPtr state);
    [StructLayout(LayoutKind.Sequential)] private struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] private static extern bool EnumWindows(Visitor visit, IntPtr state);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassNameW(IntPtr window, StringBuilder name, int capacity);
    [DllImport("user32.dll")] private static extern bool PostMessageW(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] private static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] private static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
    // Locate only this test process, never another user's editor window.
    private static IntPtr Find(uint processId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((window, state) => {
            GetWindowThreadProcessId(window, out uint owner);
            var name = new StringBuilder(128);
            GetClassNameW(window, name, name.Capacity);
            if (owner == processId && name.ToString() == "Zed::Window") { found = window; return false; }
            return true;
        }, IntPtr.Zero);
        return found;
    }
    // Capture the owned window directly; this does not capture the desktop or unrelated apps.
    public static bool Capture(uint processId, string path) {
        var window = Find(processId);
        if (window == IntPtr.Zero || !GetWindowRect(window, out Rect rect)) return false;
        using (var bitmap = new Bitmap(rect.Right - rect.Left, rect.Bottom - rect.Top)) {
            using (var graphics = Graphics.FromImage(bitmap)) {
                var dc = graphics.GetHdc();
                try { if (!PrintWindow(window, dc, 2)) return false; }
                finally { graphics.ReleaseHdc(dc); }
            }
            // Some GPU backends return success with a black hidden-window capture.
            bool painted = false;
            for (int y = 0; y < bitmap.Height && !painted; y += 16)
                for (int x = 0; x < bitmap.Width; x += 16) {
                    var pixel = bitmap.GetPixel(x, y);
                    if (pixel.R != 0 || pixel.G != 0 || pixel.B != 0) { painted = true; break; }
                }
            if (!painted) return false;
            bitmap.Save(path, ImageFormat.Png);
        }
        return true;
    }
    // Normal close preserves the final snapshot and exercises the existing worker shutdown path.
    public static bool Close(uint processId) {
        var window = Find(processId);
        return window != IntPtr.Zero && PostMessageW(window, 0x0010, IntPtr.Zero, IntPtr.Zero);
    }
}
'@
Push-Location $projectRoot
try {
    cargo run -p plugin-runtime --example svg_preview_smoke -- 'dist/plugins/svg.zip' $pluginRoot $workspace
    if ($LASTEXITCODE -ne 0) { throw 'SVG component fixture verification failed' }
    $previousPluginHome = $env:ME_EDITOR_PLUGIN_HOME
    $env:ME_EDITOR_PLUGIN_HOME = $pluginRoot
    try {
        $process = Start-Process -FilePath $hostPath -ArgumentList @('"' + $sample + '"') -WindowStyle Hidden -PassThru -RedirectStandardError "$runRoot/stderr.log" -RedirectStandardOutput "$runRoot/stdout.log"
    } finally { $env:ME_EDITOR_PLUGIN_HOME = $previousPluginHome }
    try {
        Start-Sleep -Seconds 6
        $process.Refresh()
        if ($process.HasExited) { throw "Editor exited during SVG paint: $([IO.File]::ReadAllText("$runRoot/stderr.log"))" }
        # Screenshot availability depends on the Windows GPU capture backend, not on plugin lifecycle.
        $captured = [SvgSmokeWindow]::Capture($process.Id, "$runRoot/preview.png")
        if (-not [SvgSmokeWindow]::Close($process.Id)) { throw 'Could not close the owned test window' }
        if (-not $process.WaitForExit(10000)) { throw 'Editor did not complete normal shutdown' }
        if ($process.ExitCode -ne 0) { throw "Editor failed: $([IO.File]::ReadAllText("$runRoot/stderr.log"))" }
        # Workspace-scoped instances persist state.json beneath their private workspace namespace.
        $snapshots = @(Get-ChildItem -LiteralPath "$pluginRoot/data/svg" -Recurse -File -Filter 'state.json')
        if ($snapshots.Count -eq 0) { throw 'SVG plugin snapshot was not persisted' }
        Write-Output "PASS: native SVG startup, surface lifecycle and normal shutdown; capture=$captured; logs: $runRoot"
    } finally {
        $process.Refresh()
        if (-not $process.HasExited) { Stop-Process -Id $process.Id }
    }
} finally { Pop-Location }
