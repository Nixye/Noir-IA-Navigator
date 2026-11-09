[CmdletBinding()]
param([string]$TargetDirectory, [switch]$SkipTests, [switch]$Archive, [switch]$CopyModel)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'distribution-common.ps1')
$distRoot = Join-Path $projectRoot 'dist'
New-Item -ItemType Directory -Path $distRoot -Force | Out-Null
Assert-NoirNoReparseTree -Path $distRoot
$aiAssets = Join-Path $projectRoot 'resources/ai'
if (-not (Test-Path -LiteralPath (Join-Path $aiAssets 'manifest.json'))) { & (Join-Path $projectRoot 'tools/ai/setup.ps1') }
if (-not $SkipTests) { & (Join-Path $PSScriptRoot 'test.ps1') }
& (Join-Path $PSScriptRoot 'build.ps1') -TargetDirectory $TargetDirectory
& (Join-Path $PSScriptRoot 'collect-licenses.ps1')

$buildRoot = if ($TargetDirectory) { if ([IO.Path]::IsPathRooted($TargetDirectory)) { [IO.Path]::GetFullPath($TargetDirectory) } else { [IO.Path]::GetFullPath((Join-Path $projectRoot $TargetDirectory)) } } else { Join-Path $projectRoot 'target' }
$buildOutput = Join-Path $buildRoot 'release'
$metadataPath = Join-Path $buildOutput 'noir-build.json'
$metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
$identity = Get-NoirSourceIdentity -ProjectRoot $projectRoot
if ($metadata.BuildId -ne $identity.BuildId -or $metadata.SourceSha256 -ne $identity.SourceSha256) { throw 'Build identity does not match the current sources.' }
$suffix = if ($env:OS -eq 'Windows_NT') { '.exe' } else { '' }
$binary = Join-Path $buildOutput "noir$suffix"
$cliBinary = Join-Path $buildOutput "browser-cli$suffix"
if ((Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $metadata.DesktopSha256 -or (Get-FileHash -LiteralPath $cliBinary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $metadata.CliSha256) { throw 'Build executable hashes do not match build metadata.' }
$cliVersion = (& $cliBinary --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or -not $cliVersion.Contains($identity.BuildId)) { throw 'CLI version does not identify this build. Rebuild both executables with NOIR_BUILD_ID.' }

$stage = Assert-NoirChildPath -Parent $distRoot -Path (Join-Path $distRoot ".noir-stage-$($identity.BuildId)-$([Guid]::NewGuid().ToString('N').Substring(0,8))") -DirectChild
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item -LiteralPath $binary,$cliBinary,$metadataPath -Destination $stage
Copy-Item -LiteralPath (Join-Path $projectRoot 'apps/browser-desktop/assets/noir.ico') -Destination (Join-Path $stage 'noir.ico')
foreach ($name in @('README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'THIRD_PARTY_NOTICES.md', 'Cargo.lock')) { Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination $stage }
foreach ($name in @('docs', 'fixtures', 'licenses')) { Copy-Item -LiteralPath (Join-Path $projectRoot $name) -Destination $stage -Recurse }
New-Item -ItemType Directory -Path (Join-Path $stage 'resources') -Force | Out-Null
Copy-NoirAiAssets -Source $aiAssets -Destination (Join-Path $stage 'resources/ai') -CopyModel:$CopyModel
Assert-NoirNoReparseTree -Path $stage
Write-NoirHashManifest -Directory $stage
Assert-NoirHashManifest -Directory $stage

$current = Assert-NoirChildPath -Parent $distRoot -Path (Join-Path $distRoot 'Noir') -DirectChild
if (Test-Path -LiteralPath $current) {
    Assert-NoirNoReparseTree -Path $current
    $currentPrefix = $current + [IO.Path]::DirectorySeparatorChar
    $running = Get-Process -ErrorAction SilentlyContinue | Where-Object { try { $_.Path -and $_.Path.StartsWith($currentPrefix, [StringComparison]::OrdinalIgnoreCase) } catch { $false } } | Select-Object -First 1
    if ($running) { throw "Current distribution is running (PID $($running.Id)). Validated replacement remains at $stage; close the current app before promoting." }
}
$backup = $null
try {
    if (Test-Path -LiteralPath $current) {
        $backup = Assert-NoirChildPath -Parent $distRoot -Path (Join-Path $distRoot ".noir-previous-$([DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss'))-$([Guid]::NewGuid().ToString('N').Substring(0,8))") -DirectChild
        Move-Item -LiteralPath $current -Destination $backup
    }
    Move-Item -LiteralPath $stage -Destination $current
} catch {
    if ($backup -and (Test-Path -LiteralPath $backup) -and -not (Test-Path -LiteralPath $current)) { Move-Item -LiteralPath $backup -Destination $current }
    throw
}

if ($Archive) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archivePath = Assert-NoirChildPath -Parent $distRoot -Path (Join-Path $distRoot "Noir-$([Guid]::NewGuid().ToString('N')).zip") -DirectChild
    [IO.Compression.ZipFile]::CreateFromDirectory($current, $archivePath, [IO.Compression.CompressionLevel]::Optimal, $false)
    $archivePlatform = if ($env:OS -eq 'Windows_NT') { 'windows' } else { 'linux' }
    $archiveArchitecture = switch (($metadata.Target -split '-')[0]) { 'x86_64' { 'x64' } 'aarch64' { 'arm64' } default { throw 'Unsupported distribution architecture.' } }
    $archiveName = "Noir-$archivePlatform-$archiveArchitecture.zip"
    $stableArchive = Assert-NoirChildPath -Parent $distRoot -Path (Join-Path $distRoot $archiveName) -DirectChild
    if (Test-Path -LiteralPath $stableArchive) { Assert-NoirNoReparseTree -Path $stableArchive }
    Move-Item -LiteralPath $archivePath -Destination $stableArchive -Force
    '{0}  {1}' -f (Get-FileHash -LiteralPath $stableArchive -Algorithm SHA256).Hash.ToLowerInvariant(), $archiveName | Set-Content -LiteralPath "$stableArchive.sha256" -Encoding utf8
    Write-Output $stableArchive
}
Write-Output "Current distribution: $current"
Write-Output "Build: $($identity.BuildId)"
if ($backup) { Write-Output "Previous distribution retained until cleanup: $backup" }
