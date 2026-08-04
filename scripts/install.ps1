param(
    [string]$Version = "latest",
    [string]$InstallDir = "$env:USERPROFILE\.local\bin"
)

$ErrorActionPreference = "Stop"
$repository = "beejmaxx/polymarket-mcp-rs"
if (-not [Environment]::Is64BitOperatingSystem) {
    throw "Only 64-bit Windows is supported."
}

$archive = "polymarket-mcp-rs-windows-x86_64.zip"
if ($Version -eq "latest") {
    $releaseUrl = "https://github.com/$repository/releases/latest/download"
} else {
    $tag = if ($Version.StartsWith("v")) { $Version } else { "v$Version" }
    $releaseUrl = "https://github.com/$repository/releases/download/$tag"
}

$temporaryDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid())
New-Item -ItemType Directory -Path $temporaryDir | Out-Null
try {
    $archivePath = Join-Path $temporaryDir $archive
    $checksumPath = "$archivePath.sha256"
    Invoke-WebRequest -Uri "$releaseUrl/$archive" -OutFile $archivePath
    Invoke-WebRequest -Uri "$releaseUrl/$archive.sha256" -OutFile $checksumPath

    $expected = ((Get-Content $checksumPath -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 $archivePath).Hash.ToLowerInvariant()
    if ($expected -ne $actual) {
        throw "Checksum verification failed for $archive"
    }

    Expand-Archive -Path $archivePath -DestinationPath $temporaryDir -Force
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $destination = Join-Path $InstallDir "polymarket-mcp-rs.exe"
    Copy-Item (Join-Path $temporaryDir "polymarket-mcp-rs.exe") $destination -Force
    & $destination --version
    Write-Host "Installed to $destination"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = @($userPath -split ';' | Where-Object { $_ })
    if ($pathEntries -notcontains $InstallDir) {
        $newPath = (@($pathEntries) + $InstallDir) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        Write-Host "Added $InstallDir to your user PATH. Open a new terminal to use it."
    }
} finally {
    if (Test-Path $temporaryDir) {
        Remove-Item -Recurse -Force $temporaryDir
    }
}
