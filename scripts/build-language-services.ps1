# Rebuild the two independent native-service bundles using an already available Node/npm toolchain.
# Install build dependencies in ignored task output, never globally or in a user workspace project.
param([ValidateSet('html','javascript')][string[]]$Packages = @('html','javascript'))
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path "$PSScriptRoot/..").Path
Get-Command node,npm -ErrorAction Stop | Out-Null
foreach ($name in $Packages) {
    $sourceRoot = Join-Path $projectRoot "plugins/$name/service"
    $buildRoot = Join-Path $projectRoot "target/language-services/$name"
    New-Item -ItemType Directory -Force $buildRoot | Out-Null
    Copy-Item -LiteralPath (Join-Path $sourceRoot 'package.json') -Destination $buildRoot
    Copy-Item -LiteralPath (Join-Path $sourceRoot 'server.cjs') -Destination $buildRoot
    # The source lock records the complete dependency tree; later builds require exactly those bytes.
    $lock = Join-Path $sourceRoot 'package-lock.json'
    if (Test-Path -LiteralPath $lock) { Copy-Item -LiteralPath $lock -Destination $buildRoot }
    Push-Location $buildRoot
    try {
        if (Test-Path -LiteralPath $lock) { npm ci --ignore-scripts --no-audit --no-fund }
        else { npm install --ignore-scripts --no-audit --no-fund }
        if ($LASTEXITCODE -ne 0) { throw "$name service dependency preparation failed" }
        Copy-Item -LiteralPath (Join-Path $buildRoot 'package-lock.json') -Destination $lock
        $nativeRoot = Join-Path $projectRoot "plugins/$name/native"
        New-Item -ItemType Directory -Force $nativeRoot | Out-Null
        node node_modules/esbuild/bin/esbuild server.cjs --bundle --platform=node --format=cjs --target=node24 --legal-comments=inline "--outfile=$nativeRoot/server.cjs"
        if ($LASTEXITCODE -ne 0) { throw "$name service bundling failed" }
        $licenseRoot = Join-Path $projectRoot "plugins/$name/licenses/native"
        New-Item -ItemType Directory -Force $licenseRoot | Out-Null
        # Every runtime dependency, including scoped packages, retains its original notice.
        $modules = Get-ChildItem -LiteralPath (Join-Path $buildRoot 'node_modules') -Directory | ForEach-Object {
            if ($_.Name.StartsWith('@')) { Get-ChildItem -LiteralPath $_.FullName -Directory }
            else { $_ }
        }
        $modules | ForEach-Object {
            $module = $_
            # Scope and package become a portable resource filename, with no nested path ambiguity.
            $scope = Split-Path (Split-Path $module.FullName -Parent) -Leaf
            $noticeName = if ($scope.StartsWith('@')) { "$scope-$($module.Name)" } else { $module.Name }
            # A dependency's primary license may refer to separately licensed bundled data, as TypeScript does for Unicode.
            foreach ($licenseName in @('LICENSE','LICENSE.txt','License.txt','LICENSE.md','NOTICE','NOTICE.txt','NOTICE.md','ThirdPartyNoticeText.txt')) {
                $licensePath = Join-Path $module.FullName $licenseName
                if (Test-Path -LiteralPath $licensePath -PathType Leaf) { Copy-Item -LiteralPath $licensePath -Destination (Join-Path $licenseRoot "$noticeName-$licenseName") }
            }
        }
        $manifestPath = Join-Path $projectRoot "plugins/$name/manifest.json"
        $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        foreach ($service in $manifest.services.PSObject.Properties.Value) {
            $service.installation.artifacts | Where-Object { $_.id -eq 'server' } | ForEach-Object { $_.sha256 = (Get-FileHash -LiteralPath (Join-Path $nativeRoot 'server.cjs') -Algorithm SHA256).Hash.ToLowerInvariant() }
        }
        $manifest | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $manifestPath -Encoding utf8
    } finally { Pop-Location }
}
