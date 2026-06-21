# Install lessmd from GitHub Releases (Windows).
#
#   irm https://raw.githubusercontent.com/kerloom/lessmd/master/install.ps1 | iex
#
# Environment:
#   $env:LESSMD_VERSION   Pin a release (e.g. 0.2.3 or v0.2.3). Default: latest.
#   $env:LESSMD_INSTALL   Install directory. Default: %LOCALAPPDATA%\Programs\lessmd
#   $env:LESSMD_REPO      GitHub repo slug. Default: kerloom/lessmd

$ErrorActionPreference = "Stop"

$Repo = if ($env:LESSMD_REPO) { $env:LESSMD_REPO } else { "kerloom/lessmd" }
$InstallDir = if ($env:LESSMD_INSTALL) { $env:LESSMD_INSTALL } else { Join-Path $env:LOCALAPPDATA "Programs\lessmd" }
$BinName = "lessmd.exe"

function Fail([string]$Message) {
    Write-Error "lessmd install: $Message"
}

$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
switch ($Arch) {
    "X64" { $Target = "x86_64-pc-windows-msvc" }
    "Arm64" { $Target = "aarch64-pc-windows-msvc" }
    default { Fail "unsupported Windows architecture: $Arch" }
}

if ($env:LESSMD_VERSION) {
    $Version = $env:LESSMD_VERSION.TrimStart("v")
} else {
    $Latest = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $Latest.tag_name.TrimStart("v")
}

$Tag = "v$Version"
$ArchiveName = "lessmd-$Tag-$Target.zip"
$DownloadUrl = "https://github.com/$Repo/releases/download/$Tag/$ArchiveName"
$SumsUrl = "https://github.com/$Repo/releases/download/$Tag/SHA256SUMS"

Write-Host "Installing lessmd $Tag ($Target)"

$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("lessmd-install-" + [guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Path $TempDir | Out-Null

try {
    $ArchivePath = Join-Path $TempDir $ArchiveName
    $SumsPath = Join-Path $TempDir "SHA256SUMS"

    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ArchivePath
    Invoke-WebRequest -Uri $SumsUrl -OutFile $SumsPath

    $ExpectedLine = Get-Content $SumsPath | ForEach-Object { $_.TrimEnd("`r") } | Where-Object { $_ -match " $($ArchiveName)$" } | Select-Object -First 1
    if (-not $ExpectedLine) {
        Fail "checksum for $ArchiveName not found in SHA256SUMS"
    }
    $Expected = ($ExpectedLine -split '\s+')[0]
    $Actual = (Get-FileHash -Algorithm SHA256 -Path $ArchivePath).Hash.ToLower()
    if ($Actual -ne $Expected) {
        Fail "checksum mismatch for $ArchiveName"
    }

    Expand-Archive -Path $ArchivePath -DestinationPath $TempDir -Force
    $BinaryPath = Join-Path $TempDir $BinName
    if (-not (Test-Path $BinaryPath)) {
        Fail "archive did not contain $BinName"
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path $BinaryPath -Destination (Join-Path $InstallDir $BinName) -Force

    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$InstallDir;$UserPath", "User")
        $env:Path = "$InstallDir;$env:Path"
        Write-Host "Added $InstallDir to your user PATH (open a new terminal if needed)."
    }

    Write-Host ""
    Write-Host "lessmd $Tag installed to $(Join-Path $InstallDir $BinName)"
} finally {
    Remove-Item -Recurse -Force $TempDir
}
