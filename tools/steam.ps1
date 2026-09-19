# Usage: ./tools/steam.ps1 run [--join-lobby ID] [--release]
$ErrorActionPreference = 'Stop'
python (Join-Path $PSScriptRoot 'steam.py') @args
exit $LASTEXITCODE
