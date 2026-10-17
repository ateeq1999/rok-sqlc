# Install rok-sqlc from a GitHub release on Windows.
#
#   irm https://raw.githubusercontent.com/ateeq1999/rok-sqlc/main/install.ps1 | iex
#
# To pick a release or directory, set the environment variables first:
#
#   $env:ROK_SQLC_VERSION = "v0.2.1"                      # default: latest
#   $env:ROK_SQLC_INSTALL_DIR = "$HOME\.rok-sqlc\bin"     # the default
#   $env:ROK_SQLC_NO_MODIFY_PATH = "1"                    # leave the user PATH alone
#
# The script downloads the archive for this machine's architecture, checks
# it against the release's SHA-256 checksum, copies rok-sqlc.exe into the
# install directory, and adds that directory to the user PATH.

# Everything runs inside a function so that, under `irm | iex`, the script's
# variables and $ErrorActionPreference stay out of the caller's session.
function Install-RokSqlc {
    $ErrorActionPreference = "Stop"
    $ProgressPreference = "SilentlyContinue"

    $Repo = "ateeq1999/rok-sqlc"
    # Where release assets are downloaded from: <base>/<tag>/<file>.
    $BaseUrl = if ($env:ROK_SQLC_BASE_URL) { $env:ROK_SQLC_BASE_URL } else { "https://github.com/$Repo/releases/download" }
    $Version = if ($env:ROK_SQLC_VERSION) { $env:ROK_SQLC_VERSION } else { "latest" }
    $InstallDir = if ($env:ROK_SQLC_INSTALL_DIR) { $env:ROK_SQLC_INSTALL_DIR } else { Join-Path $HOME ".rok-sqlc\bin" }

    # Throws rather than exits: under `irm | iex`, `exit` would close the
    # user's PowerShell window.
    function Fail($Message) {
        throw "rok-sqlc-install: $Message"
    }

    # Windows PowerShell 5.1 defaults to TLS 1.0, which GitHub refuses.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $Arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $Arch) { $Arch = $env:PROCESSOR_ARCHITECTURE }
    switch ($Arch) {
        "AMD64" { $Target = "x86_64-pc-windows-msvc" }
        "ARM64" { $Target = "aarch64-pc-windows-msvc" }
        default { Fail "unsupported architecture: $Arch (build from source with cargo install rok-sqlc)" }
    }

    if ($Version -eq "latest") {
        # releases/latest redirects to releases/tag/<tag>; reading the redirect
        # avoids the GitHub API and its rate limit.
        $Request = [Net.WebRequest]::Create("https://github.com/$Repo/releases/latest")
        $Request.AllowAutoRedirect = $false
        try {
            $Response = $Request.GetResponse()
            $Location = $Response.Headers["Location"]
            $Response.Close()
        } catch {
            Fail "could not look up the latest release: $_"
        }
        $Tag = ($Location -split "/")[-1]
        if ($Tag -notmatch "^v[0-9]") { Fail "could not find a published release of rok-sqlc" }
    } elseif ($Version.StartsWith("v")) {
        $Tag = $Version
    } else {
        $Tag = "v$Version"
    }

    $Name = "rok-sqlc-$($Tag.Substring(1))-$Target"
    $Archive = "$Name.zip"
    $Temp = Join-Path ([IO.Path]::GetTempPath()) ("rok-sqlc-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Force $Temp | Out-Null

    try {
        Write-Output "rok-sqlc-install: downloading rok-sqlc $($Tag.Substring(1)) for $Target"
        try {
            Invoke-WebRequest -UseBasicParsing "$BaseUrl/$Tag/$Archive" -OutFile "$Temp\$Archive"
            Invoke-WebRequest -UseBasicParsing "$BaseUrl/$Tag/$Archive.sha256" -OutFile "$Temp\$Archive.sha256"
        } catch {
            Fail "download failed: $BaseUrl/$Tag/$Archive (is $Tag a release with a $Target build?)"
        }

        $Expected = ((Get-Content -Raw "$Temp\$Archive.sha256").Trim() -split "\s+")[0].ToLower()
        $Actual = (Get-FileHash -Algorithm SHA256 "$Temp\$Archive").Hash.ToLower()
        if (-not $Expected -or $Expected -ne $Actual) {
            Fail "checksum mismatch for $Archive (expected $Expected, got $Actual)"
        }

        Expand-Archive -Force "$Temp\$Archive" -DestinationPath $Temp
        $Exe = Join-Path $Temp "$Name\rok-sqlc.exe"
        if (-not (Test-Path $Exe)) { Fail "$Archive does not contain rok-sqlc.exe" }

        New-Item -ItemType Directory -Force $InstallDir | Out-Null
        Copy-Item -Force $Exe (Join-Path $InstallDir "rok-sqlc.exe")
    } finally {
        Remove-Item -Recurse -Force $Temp -ErrorAction SilentlyContinue
    }

    $Installed = Join-Path $InstallDir "rok-sqlc.exe"
    Write-Output "rok-sqlc-install: installed $(& $Installed --version) to $Installed"

    if ($env:ROK_SQLC_NO_MODIFY_PATH) { return }
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $Entries = if ($UserPath) { $UserPath -split ";" } else { @() }
    if ($Entries -notcontains $InstallDir) {
        $NewPath = (@($Entries | Where-Object { $_ }) + $InstallDir) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        $env:Path = "$env:Path;$InstallDir"
        Write-Output "rok-sqlc-install: added $InstallDir to your user PATH; restart your terminal to pick it up"
    }
}

Install-RokSqlc
