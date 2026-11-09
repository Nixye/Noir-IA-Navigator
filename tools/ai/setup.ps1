[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$manifest = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'manifest.json') -Raw | ConvertFrom-Json
$destination = Join-Path $projectRoot 'resources/ai'
$runtime = Join-Path $destination 'runtime'
$models = Join-Path $destination 'models'
$downloads = Join-Path $projectRoot 'target/ai-downloads'
New-Item -ItemType Directory -Path $runtime,$models,$downloads -Force | Out-Null
$windowsHost = [System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT
$downloadCommand = if ($windowsHost) { 'curl.exe' } else { 'curl' }

function Get-VerifiedAsset([string]$Uri, [string]$Path, [string]$Sha256) {
    if ((Test-Path -LiteralPath $Path) -and (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -eq $Sha256) { return }
    $partial = "$Path.partial"
    & $downloadCommand --fail --location --retry 2 --silent --show-error --output $partial $Uri
    if ($LASTEXITCODE -ne 0) { throw "Download failed: $Uri" }
    if ((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $Sha256) { throw "SHA-256 mismatch: $Uri" }
    Move-Item -LiteralPath $partial -Destination $Path -Force
}

if ($windowsHost) {
    $archive = Join-Path $downloads 'llama-runtime.zip'
    Get-VerifiedAsset $manifest.windows_url $archive $manifest.windows_sha256
    Expand-Archive -LiteralPath $archive -DestinationPath $runtime -Force
} else {
    $archive = Join-Path $downloads 'llama-runtime.tar.gz'
    Get-VerifiedAsset $manifest.linux_url $archive $manifest.linux_sha256
    & tar -xzf $archive -C $runtime
    if ($LASTEXITCODE -ne 0) { throw 'Could not extract llama.cpp runtime' }
}
Get-VerifiedAsset $manifest.model_url (Join-Path $models $manifest.model_file) $manifest.model_sha256
$modelLicense = "https://huggingface.co/$($manifest.model_repository)/resolve/$($manifest.model_revision)/LICENSE"
Invoke-WebRequest -Uri $modelLicense -OutFile (Join-Path $destination 'LICENSE-QWEN.txt')
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/ggml-org/llama.cpp/$($manifest.runtime_version)/LICENSE" -OutFile (Join-Path $destination 'LICENSE-LLAMA.txt')
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'manifest.json') -Destination (Join-Path $destination 'manifest.json')
$hashes = Get-ChildItem -LiteralPath $runtime,$models -File -Recurse | Sort-Object FullName | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant(), $_.FullName.Substring($destination.Length + 1).Replace([char]92,[char]47)
}
$hashes | Set-Content -LiteralPath (Join-Path $destination 'SHA256SUMS') -Encoding utf8
Write-Output "Verified bundled AI assets: $destination"
