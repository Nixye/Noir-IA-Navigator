$ErrorActionPreference = 'Stop'

function Get-NoirVersion {
    param([Parameter(Mandatory)][string]$ProjectRoot)
    $cargo = Get-Content -LiteralPath (Join-Path $ProjectRoot 'Cargo.toml') -Raw
    $match = [regex]::Match($cargo, '(?ms)^\[workspace\.package\]\s*.*?^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) { throw 'Workspace version is missing.' }
    $match.Groups[1].Value
}

function Get-NoirSourceIdentity {
    param([Parameter(Mandatory)][string]$ProjectRoot)
    $paths = [System.Collections.Generic.List[string]]::new()
    foreach ($directory in @('apps', 'crates', 'fixtures')) {
        Get-ChildItem -LiteralPath (Join-Path $ProjectRoot $directory) -File -Recurse | ForEach-Object {
            if ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Source links are not supported in build identity: $($_.FullName)" }
            $paths.Add($_.FullName.Substring($ProjectRoot.Length + 1).Replace([char]92, [char]47))
        }
    }
    foreach ($relative in @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'tools/ai/manifest.json', 'scripts/build.ps1', 'scripts/package.ps1', 'scripts/distribution-common.ps1')) {
        if (Test-Path -LiteralPath (Join-Path $ProjectRoot $relative) -PathType Leaf) { $paths.Add($relative) }
    }
    $paths.Sort([StringComparer]::Ordinal)
    $lines = foreach ($relative in $paths) {
        '{0}  {1}' -f (Get-FileHash -LiteralPath (Join-Path $ProjectRoot $relative) -Algorithm SHA256).Hash.ToLowerInvariant(), $relative
    }
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $digest = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($lines -join "`n")))).Replace('-', '').ToLowerInvariant() }
    finally { $sha.Dispose() }
    $version = Get-NoirVersion -ProjectRoot $ProjectRoot
    [pscustomobject]@{ Version = $version; SourceSha256 = $digest; BuildId = "$version-$($digest.Substring(0, 12))" }
}

function Assert-NoirChildPath {
    param([Parameter(Mandatory)][string]$Parent, [Parameter(Mandatory)][string]$Path, [switch]$DirectChild)
    $parentPath = [IO.Path]::GetFullPath($Parent).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    $targetPath = [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    if (-not $targetPath.StartsWith($parentPath + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Path is outside the intended directory: $targetPath" }
    if ($DirectChild -and -not [IO.Path]::GetDirectoryName($targetPath).Equals($parentPath, [StringComparison]::OrdinalIgnoreCase)) { throw "Path is not a direct child of the intended directory: $targetPath" }
    # Lexical containment alone cannot detect a junction in an ancestor. Reject
    # linked ancestors before any destructive promotion or cleanup operation.
    $ancestor = $targetPath
    while ($ancestor) {
        if (Test-Path -LiteralPath $ancestor) {
            $item = Get-Item -LiteralPath $ancestor -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing a linked path ancestor: $ancestor" }
        }
        $next = [IO.Path]::GetDirectoryName($ancestor)
        if ($next -eq $ancestor) { break }
        $ancestor = $next
    }
    $targetPath
}

function Assert-NoirNoReparseTree {
    param([Parameter(Mandatory)][string]$Path)
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing a linked target: $Path" }
    if ($item.PSIsContainer) {
        $link = Get-ChildItem -LiteralPath $Path -Recurse -Force | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint } | Select-Object -First 1
        if ($link) { throw "Refusing a directory containing a link: $($link.FullName)" }
    }
}

function Write-NoirHashManifest {
    param([Parameter(Mandatory)][string]$Directory)
    $manifest = Join-Path $Directory 'SHA256SUMS'
    $lines = Get-ChildItem -LiteralPath $Directory -File -Recurse | Where-Object { $_.FullName -ne $manifest } | Sort-Object FullName | ForEach-Object {
        $relative = $_.FullName.Substring($Directory.Length + 1).Replace([char]92, [char]47)
        '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $relative
    }
    $lines | Set-Content -LiteralPath $manifest -Encoding utf8
}

function Assert-NoirHashManifest {
    param([Parameter(Mandatory)][string]$Directory)
    Assert-NoirNoReparseTree -Path $Directory
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($line in Get-Content -LiteralPath (Join-Path $Directory 'SHA256SUMS')) {
        if ($line -notmatch '^([a-f0-9]{64})  (.+)$') { throw 'Invalid package hash manifest line.' }
        $expected = $Matches[1]; $relative = $Matches[2]
        if ([IO.Path]::IsPathRooted($relative) -or -not $seen.Add($relative)) { throw "Invalid or repeated manifest path: $relative" }
        $path = Assert-NoirChildPath -Parent $Directory -Path (Join-Path $Directory $relative)
        if ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expected) { throw "Package hash mismatch: $relative" }
    }
    if ($seen.Count -eq 0) { throw 'Empty package hash manifest.' }
    foreach ($file in Get-ChildItem -LiteralPath $Directory -File -Recurse) {
        $relative = $file.FullName.Substring($Directory.Length + 1).Replace([char]92, [char]47)
        if ($relative -ne 'SHA256SUMS' -and -not $seen.Contains($relative)) { throw "File is absent from the package hash manifest: $relative" }
    }
}

function Copy-NoirAiAssets {
    param([Parameter(Mandatory)][string]$Source, [Parameter(Mandatory)][string]$Destination, [switch]$CopyModel)
    Assert-NoirNoReparseTree -Path $Source
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    foreach ($directory in Get-ChildItem -LiteralPath $Source -Directory -Recurse) {
        New-Item -ItemType Directory -Path (Join-Path $Destination $directory.FullName.Substring($Source.Length + 1)) -Force | Out-Null
    }
    foreach ($file in Get-ChildItem -LiteralPath $Source -File -Recurse) {
        $target = Join-Path $Destination $file.FullName.Substring($Source.Length + 1)
        $linked = $false
        if (-not $CopyModel -and $file.Extension -eq '.gguf' -and $env:OS -eq 'Windows_NT') {
            try { New-Item -ItemType HardLink -Path $target -Value $file.FullName -ErrorAction Stop | Out-Null; $linked = $true } catch { $linked = $false }
        }
        if (-not $linked) { Copy-Item -LiteralPath $file.FullName -Destination $target }
    }
}
