[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
Push-Location -LiteralPath $projectRoot
try {
    & cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'Formatting check failed.' }
    & cargo clippy --workspace --all-targets --locked -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Clippy failed.' }
    & cargo test --workspace --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tests failed.' }
} finally {
    Pop-Location
}
