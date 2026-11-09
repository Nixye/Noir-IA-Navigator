[CmdletBinding()]
param([string]$Target)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
Push-Location -LiteralPath $projectRoot
try {
    if (-not $Target) {
        $version = & rustc -vV
        if ($LASTEXITCODE -ne 0) { throw 'Could not determine Rust host target.' }
        $hostLine = $version | Where-Object { $_ -like 'host: *' }
        $Target = $hostLine.Substring(6).Trim()
    }
    if ($Target -notmatch '^[A-Za-z0-9_.-]+$') { throw 'Invalid target triple.' }
    $metadataText = & cargo metadata --locked --format-version 1 --filter-platform $Target
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
    $metadata = $metadataText | ConvertFrom-Json
    $licensesRoot = Join-Path $projectRoot 'licenses'
    $dependencyRoot = Join-Path $licensesRoot 'dependencies'
    New-Item -ItemType Directory -Path $dependencyRoot -Force | Out-Null
    $inventory = foreach ($package in $metadata.packages | Where-Object { $_.source } | Sort-Object name,version) {
        $packageRoot = Split-Path -Parent $package.manifest_path
        $packageDestination = Join-Path $dependencyRoot "$($package.name)-$($package.version)"
        New-Item -ItemType Directory -Path $packageDestination -Force | Out-Null
        $candidates = @(Get-ChildItem -LiteralPath $packageRoot -File | Where-Object {
            $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE)([.\-_].*)?$|^UNLICENSE$'
        })
        if ($package.license_file) {
            $declaredLicense = Join-Path $packageRoot $package.license_file
            if (Test-Path -LiteralPath $declaredLicense -PathType Leaf) {
                $candidates += Get-Item -LiteralPath $declaredLicense
            }
        }
        $copied = @($candidates | Sort-Object FullName -Unique | ForEach-Object {
            Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $packageDestination $_.Name) -Force
            $_.Name
        })
        [pscustomobject]@{
            Name = $package.name
            Version = $package.version
            License = $package.license
            Repository = $package.repository
            RegistryLicenseFiles = $copied -join ';'
            PackagedLicenseFiles = @(Get-ChildItem -LiteralPath $packageDestination -File | Sort-Object Name | ForEach-Object { $_.Name }) -join ';'
        }
    }
    $inventoryPath = Join-Path $licensesRoot "dependency-inventory-$Target.csv"
    $inventory | Export-Csv -LiteralPath $inventoryPath -NoTypeInformation -Encoding utf8
    Write-Output "Dependency metadata recorded: $inventoryPath"
    Write-Output "Packages: $(@($inventory).Count); package licenses require review when absent or ambiguous."
} finally {
    Pop-Location
}
