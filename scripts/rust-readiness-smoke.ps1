# Build first, then release Cargo's target lock before rust-analyzer loads this workspace.
$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
    $messages = cargo test -p editor-app --no-run --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Readiness test build failed' }
    $testBinary = $messages | ForEach-Object {
        $message = $_ | ConvertFrom-Json
        if ($message.reason -eq 'compiler-artifact' -and $message.profile.test -and $message.executable) {
            $message.executable
        }
    } | Select-Object -Last 1
    if (!$testBinary) { throw 'Readiness test executable was not produced' }
    & $testBinary --exact language::navigation::readiness_tests::local_rust_server_readiness --ignored --nocapture
    if ($LASTEXITCODE -ne 0) { throw 'Rust language server did not become ready' }
} finally {
    Pop-Location
}
