[CmdletBinding()]
param([switch]$Apply, [switch]$IncludeStaleBuildBinaries, [string[]]$Names)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'distribution-common.ps1')
$distRoot = Join-Path $projectRoot 'dist'
$current = Assert-NoirChildPath -Parent $distRoot -Path (Join-Path $distRoot 'Noir') -DirectChild
if ($Apply) {
    if (-not (Test-Path -LiteralPath $current -PathType Container)) { throw 'A verified dist/Noir replacement must exist before cleanup.' }
    Assert-NoirNoReparseTree -Path $current
    Assert-NoirHashManifest -Directory $current
    $metadata = Get-Content -LiteralPath (Join-Path $current 'noir-build.json') -Raw | ConvertFrom-Json
    $currentExe = Join-Path $current 'noir.exe'
    if ((Get-FileHash -LiteralPath $currentExe -Algorithm SHA256).Hash.ToLowerInvariant() -ne $metadata.DesktopSha256) { throw 'Current executable does not match its build identity.' }
}

$candidates = [Collections.Generic.List[object]]::new()
if (Test-Path -LiteralPath $distRoot -PathType Container) {
    $distItem = Get-Item -LiteralPath $distRoot -Force
    if ($distItem.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Refusing a linked distribution directory.' }
    foreach ($item in Get-ChildItem -LiteralPath $distRoot -Force) {
        $legacyDirectory = $item.PSIsContainer -and ($item.Name -match '^noir-\d{8}-\d{6}-\d{3}$' -or $item.Name -match '^\.noir-previous-\d{8}-\d{6}-[a-f0-9]{8}$')
        $legacyArchive = -not $item.PSIsContainer -and $item.Name -match '^noir-\d+\.\d+\.\d+-windows-x64\.zip(?:\.sha256)?$'
        if (-not ($legacyDirectory -or $legacyArchive)) { continue }
        if ($Names -and $item.Name -notin $Names) { continue }
        $absolute = Assert-NoirChildPath -Parent $distRoot -Path $item.FullName -DirectChild
        Assert-NoirNoReparseTree -Path $absolute
        if ($legacyDirectory -and -not (Test-Path -LiteralPath (Join-Path $absolute 'noir.exe') -PathType Leaf)) { throw "Refusing an unrecognized directory: $absolute" }
        $bytes = if ($item.PSIsContainer) { (Get-ChildItem -LiteralPath $absolute -File -Recurse | Measure-Object Length -Sum).Sum } else { $item.Length }
        $candidates.Add([pscustomobject]@{ Name=$item.Name; Path=$absolute; Kind=if ($item.PSIsContainer) { 'Directory' } else { 'File' }; Bytes=[long]$bytes })
    }
}

if ($IncludeStaleBuildBinaries) {
    $metadataPath = Join-Path $current 'noir-build.json'
    $metadata = if (Test-Path -LiteralPath $metadataPath) { Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json } else { $null }
    foreach ($profile in @('debug','release')) {
        foreach ($binary in @('noir.exe','browser-cli.exe')) {
            $relative = "target/$profile/$binary"
            if ($Names -and $relative -notin $Names) { continue }
            $path = Assert-NoirChildPath -Parent $projectRoot -Path (Join-Path $projectRoot $relative)
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }
            Assert-NoirNoReparseTree -Path (Split-Path -Parent $path)
            $expected = if ($metadata) { if ($binary -eq 'noir.exe') { $metadata.DesktopSha256 } else { $metadata.CliSha256 } } else { $null }
            if ($expected -and (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -eq $expected) { continue }
            $candidates.Add([pscustomobject]@{ Name=$relative; Path=$path; Kind='File'; Bytes=(Get-Item -LiteralPath $path).Length })
        }
    }
}

if ($Names) {
    foreach ($name in $Names) { if ($name -notin $candidates.Name) { throw "Name is not a recognized stale artifact: $name" } }
}
if ($Apply) {
    $processPaths = @(Get-Process -ErrorAction SilentlyContinue | ForEach-Object { try { if ($_.Path) { [pscustomobject]@{ Id=$_.Id; Path=$_.Path } } } catch { } })
    foreach ($candidate in $candidates) {
        $running = $processPaths | Where-Object { $_.Path.Equals($candidate.Path,[StringComparison]::OrdinalIgnoreCase) -or ($candidate.Kind -eq 'Directory' -and $_.Path.StartsWith($candidate.Path + [IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)) } | Select-Object -First 1
        if ($running) { throw "A stale artifact is still running (PID $($running.Id)): $($candidate.Path). No artifacts have been removed." }
    }
    foreach ($candidate in $candidates) {
        # All final absolute paths have been checked against explicit workspace
        # directories, and all links/running processes were refused above.
        if ($candidate.Kind -eq 'Directory') { Remove-Item -LiteralPath $candidate.Path -Recurse -Force }
        else { Remove-Item -LiteralPath $candidate.Path -Force }
    }
}
$candidates | Select-Object Name,Path,Kind,Bytes
Write-Output $(if ($Apply) { "Removed $($candidates.Count) stale artifacts. dist/Noir, resources/ai, source, profiles and Cargo dependencies were preserved." } else { "Dry run: $($candidates.Count) stale artifacts. Nothing was removed. Use -Apply only after the replacement is verified." })
