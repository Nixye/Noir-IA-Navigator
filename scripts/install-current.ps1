[CmdletBinding()]
param([switch]$Desktop, [switch]$StartMenu)

$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'Windows shortcuts are supported only on Windows.' }
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'distribution-common.ps1')
$current = Assert-NoirChildPath -Parent (Join-Path $projectRoot 'dist') -Path (Join-Path $projectRoot 'dist/Noir') -DirectChild
Assert-NoirNoReparseTree -Path $current
$executable = Join-Path $current 'noir.exe'
$metadata = Get-Content -LiteralPath (Join-Path $current 'noir-build.json') -Raw | ConvertFrom-Json
if ((Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant() -ne $metadata.DesktopSha256) { throw 'The current executable does not match build metadata.' }
if (-not $Desktop -and -not $StartMenu) { $Desktop = $true; $StartMenu = $true }
$locations = @()
if ($Desktop) { $locations += Join-Path ([Environment]::GetFolderPath('DesktopDirectory')) 'Noir.lnk' }
if ($StartMenu) { $locations += Join-Path ([Environment]::GetFolderPath('Programs')) 'Noir/Noir.lnk' }
$shellLink = New-Object -ComObject WScript.Shell
foreach ($location in $locations) {
    if (Test-Path -LiteralPath $location) {
        $existing = $shellLink.CreateShortcut($location)
        if (-not $existing.TargetPath.StartsWith($projectRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw "Refusing to replace an unrelated shortcut: $location" }
    }
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($location)) -Force | Out-Null
    $shortcut = $shellLink.CreateShortcut($location)
    $shortcut.TargetPath = $executable
    $shortcut.WorkingDirectory = $current
    $shortcut.Arguments = ''
    $shortcut.Description = "Noir $($metadata.Version) — $($metadata.BuildId)"
    $iconPath = Join-Path $current 'noir.ico'
    $shortcut.IconLocation = if (Test-Path -LiteralPath $iconPath -PathType Leaf) { "$iconPath,0" } else { "$executable,0" }
    $shortcut.Save()
    Write-Output "Shortcut: $location -> $executable"
}
