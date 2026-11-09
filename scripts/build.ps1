[CmdletBinding()]
param([switch]$DebugBuild, [string]$TargetDirectory)

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'distribution-common.ps1')
$identity = Get-NoirSourceIdentity -ProjectRoot $projectRoot
$previousBuildId = [Environment]::GetEnvironmentVariable('NOIR_BUILD_ID', 'Process')
Push-Location -LiteralPath $projectRoot
try {
    $env:NOIR_BUILD_ID = $identity.BuildId
    $buildArguments = @('build','--workspace','--locked')
    if (-not $DebugBuild) { $buildArguments += '--release' }
    if ($TargetDirectory) { $buildArguments += @('--target-dir',$TargetDirectory) }
    & cargo @buildArguments
    if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
    $after = Get-NoirSourceIdentity -ProjectRoot $projectRoot
    if ($after.SourceSha256 -ne $identity.SourceSha256) { throw 'Sources changed during compilation. Rebuild after edits finish; no distribution identity was written.' }
    $buildRoot = if ($TargetDirectory) { if ([IO.Path]::IsPathRooted($TargetDirectory)) { [IO.Path]::GetFullPath($TargetDirectory) } else { [IO.Path]::GetFullPath((Join-Path $projectRoot $TargetDirectory)) } } else { Join-Path $projectRoot 'target' }
    $profile = if ($DebugBuild) { 'debug' } else { 'release' }
    $output = Join-Path $buildRoot $profile
    $suffix = if ($env:OS -eq 'Windows_NT') { '.exe' } else { '' }
    $desktop = Join-Path $output "noir$suffix"; $cli = Join-Path $output "browser-cli$suffix"
    $compilerInfo = @(& rustc -vV)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot record compiler version.' }
    $compiler = $compilerInfo[0]
    $hostLine = $compilerInfo | Where-Object { $_ -like 'host: *' } | Select-Object -First 1
    if (-not $hostLine) { throw 'Compiler host target is missing.' }
    [ordered]@{
        Version = $identity.Version; BuildId = $identity.BuildId; SourceSha256 = $identity.SourceSha256
        DesktopSha256 = (Get-FileHash -LiteralPath $desktop -Algorithm SHA256).Hash.ToLowerInvariant()
        CliSha256 = (Get-FileHash -LiteralPath $cli -Algorithm SHA256).Hash.ToLowerInvariant()
        Compiler = $compiler; Target = $hostLine.Substring(6).Trim(); BuiltAtUtc = [DateTime]::UtcNow.ToString('O'); Configuration = $profile
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $output 'noir-build.json') -Encoding utf8
    Write-Output "Build: $($identity.BuildId)"
} finally {
    [Environment]::SetEnvironmentVariable('NOIR_BUILD_ID', $previousBuildId, 'Process')
    Pop-Location
}
