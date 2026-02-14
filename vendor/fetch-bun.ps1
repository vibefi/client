# Downloads Bun binary for bundling into packaged Windows apps.
# Usage: .\vendor\fetch-bun.ps1 [-Version 1.3.7]
# Output: vendor\bun\bun-x86_64-pc-windows-gnu.exe

param(
    [string]$Version = "1.3.7"
)

$ErrorActionPreference = "Stop"

$VendorDir = Join-Path $PSScriptRoot "bun"
if (-not (Test-Path $VendorDir)) {
    New-Item -ItemType Directory -Path $VendorDir -Force | Out-Null
}

$BunTriples = @("bun-windows-x64-baseline", "bun-windows-x64")
$OutPath = Join-Path $VendorDir "bun-x86_64-pc-windows-gnu.exe"
$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) ("fetch-bun-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

$downloaded = $false

try {
    foreach ($triple in $BunTriples) {
        $url = "https://github.com/oven-sh/bun/releases/download/bun-v${Version}/${triple}.zip"
        $zipPath = Join-Path $TmpDir "bun.zip"

        Write-Host "Downloading bun ${Version} for windows-x64 (${triple})..."
        try {
            Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing -ErrorAction Stop
        } catch {
            Write-Host "  failed, trying next variant..."
            continue
        }

        Expand-Archive -Path $zipPath -DestinationPath $TmpDir -Force
        $bunExe = Join-Path (Join-Path $TmpDir $triple) "bun.exe"
        if (-not (Test-Path $bunExe)) {
            Write-Host "  bun.exe not found in archive, trying next variant..."
            continue
        }

        Copy-Item -Path $bunExe -Destination $OutPath -Force
        $downloaded = $true
        break
    }

    if (-not $downloaded) {
        Write-Error "Failed to download bun ${Version} for windows-x64"
        exit 1
    }

    Write-Host "  -> $OutPath"
    Write-Host "Done. Vendored Bun binary is ready."
} finally {
    Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue
}
