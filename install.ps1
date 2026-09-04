# ==============================================================================
# InterMCP 1-Click Universal Installer for Windows PowerShell
# https://github.com/Bharathcoorg/intermcp
# ==============================================================================

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

Write-Host "`n⚡ InterMCP 1-Click Windows Installer" -ForegroundColor Cyan
Write-Host "============================================================"

$InstallDir = Join-Path $HOME ".intermcp\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$BinaryName = "intermcp.exe"
$BinaryPath = Join-Path $InstallDir $BinaryName
$Repo = "Bharathcoorg/intermcp"

Write-Host "📁 Target installation path: $BinaryPath" -ForegroundColor Gray

# If local cargo and Cargo.toml exist, build from source
if ((Get-Command "cargo" -ErrorAction SilentlyContinue) -and (Test-Path "Cargo.toml")) {
    Write-Host "`n🔨 Building native optimized binary using local Rust toolchain..." -ForegroundColor Yellow
    cargo build --release
    Copy-Item "target\release\$BinaryName" -Destination $BinaryPath -Force
} else {
    $ZipUrl = "https://github.com/$Repo/releases/latest/download/intermcp-windows-x86_64.zip"
    $TempZip = Join-Path $env:TEMP "intermcp.zip"

    Write-Host "`n📥 Downloading release package from: $ZipUrl" -ForegroundColor Yellow
    try {
        Invoke-WebRequest -Uri $ZipUrl -OutFile $TempZip -UseBasicParsing
        Expand-Archive -Path $TempZip -DestinationPath $InstallDir -Force
        Remove-Item $TempZip -Force
    } catch {
        if (Get-Command "cargo" -ErrorAction SilentlyContinue) {
            Write-Host "Prebuilt binary download not available. Compiling via cargo install..." -ForegroundColor Yellow
            cargo install intermcp --root (Join-Path $HOME ".intermcp")
        } else {
            Write-Host "❌ Could not download prebuilt release and Rust 'cargo' is not installed." -ForegroundColor Red
            Write-Host "Please visit https://github.com/$Repo/releases to download manually." -ForegroundColor Red
            exit 1
        }
    }
}

# Add to User PATH if not already present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "✅ Added $InstallDir to User PATH." -ForegroundColor Green
}

Write-Host "`n✅ InterMCP binary installed successfully!" -ForegroundColor Green

# Automatically configure all desktop AI IDEs
Write-Host "`n⚡ Running 1-Click IDE Auto-Configuration..." -ForegroundColor Cyan
& $BinaryPath setup

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "🎉 Installation and IDE Setup Complete!" -ForegroundColor Green
Write-Host "Restart your IDE (Antigravity, Cursor, Claude, Kilo Code, VS Code) to start using your tools.`n"
