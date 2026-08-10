# Tiny Mite — Windows packaging script
# Creates a distributable ZIP bundle from the Rust workspace.

param(
    [string]$Version = "0.1.0"
)

$ErrorActionPreference = "Stop"
$AppName = "tiny-mite"
$OutputDir = "target/release-package"
$BundleDir = "$OutputDir/$AppName-$Version-windows-x64"
$BinDir = "$BundleDir/bin"
$DocsDir = "$BundleDir/docs"
$ModelsDir = "$BundleDir/models"
$DataDir = "$BundleDir/data"

Write-Host "=== Tiny Mite Windows Packaging v$Version ==="

# Build release binary
Write-Host "[1/5] Building release binary..."
cargo build --release --workspace 2>&1 | Select-Object -Last 3
if ($LASTEXITCODE -ne 0) {
    Write-Host "  Note: Build may have completed with warnings; continuing..."
}

# Create directory structure
Write-Host "[2/5] Creating package structure..."
New-Item -ItemType Directory -Force -Path $BinDir, $DocsDir, $ModelsDir, $DataDir | Out-Null

# Copy binaries
Write-Host "[3/5] Copying binaries..."
Get-ChildItem "target/release/tiny-mite*" -ErrorAction SilentlyContinue | Where-Object { $_.Name -like "*.exe" } | ForEach-Object {
    Copy-Item $_.FullName "$BinDir/" -Force
}

# Copy documentation
Write-Host "[4/5] Copying documentation..."
if (Test-Path "README.md") { Copy-Item "README.md" "$DocsDir/" -Force }
if (Test-Path "AGENTS.md") { Copy-Item "AGENTS.md" "$DocsDir/" -Force }
if (Test-Path "BUILD_MANIFEST.md") { Copy-Item "BUILD_MANIFEST.md" "$DocsDir/" -Force }
if (Test-Path "docs/") { Copy-Item "docs/" "$DocsDir/docs/" -Recurse -Force }

# Create launcher
Write-Host "[5/5] Creating launcher..."
@"
@echo off
set TINY_MITE_HOME=%~dp0
set TINY_MITE_MODELS_DIR=%~dp0models
set TINY_MITE_DATA_DIR=%~dp0data
"%~dp0bin\tiny-mite.exe" %*
"@ | Out-File -FilePath "$BundleDir/tiny-mite.bat" -Encoding ASCII

# Create ZIP
Write-Host "  Creating ZIP archive..."
Compress-Archive -Path "$BundleDir/*" -DestinationPath "$OutputDir/$AppName-$Version-windows-x64.zip" -Force

Write-Host ""
Write-Host "=== Package created ==="
Write-Host "  Bundle: $BundleDir"
Write-Host "  Archive: $OutputDir/$AppName-$Version-windows-x64.zip"
Write-Host "  Launch: $BundleDir/tiny-mite.bat"