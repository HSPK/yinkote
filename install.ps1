# Works in Windows PowerShell 5.1 and PowerShell 7, including "irm ... | iex".
& {
    Set-StrictMode -Version 2
    $ErrorActionPreference = 'Stop'
    $repo = 'https://github.com/HSPK/yinkote'
    $architecture = $env:PROCESSOR_ARCHITEW6432
    if ([string]::IsNullOrEmpty($architecture)) { $architecture = $env:PROCESSOR_ARCHITECTURE }
    if ($architecture -ne 'AMD64') {
        throw "Only Windows x64 binaries are published; detected architecture: $architecture"
    }

    $protocol = [Net.ServicePointManager]::SecurityProtocol
    $staging = $null
    try {
        [Net.ServicePointManager]::SecurityProtocol = $protocol -bor [Net.SecurityProtocolType]::Tls12
        $response = Invoke-WebRequest -UseBasicParsing -Uri "$repo/releases/latest" -Method Head -UserAgent 'yinkote-installer' -TimeoutSec 300
        if ($response.BaseResponse.PSObject.Properties['ResponseUri']) {
            $latest = $response.BaseResponse.ResponseUri.AbsoluteUri
        } else {
            $latest = $response.BaseResponse.RequestMessage.RequestUri.AbsoluteUri
        }
        if ($latest -notmatch ('^' + [regex]::Escape($repo) + '/releases/tag/(v[0-9]+\.[0-9]+\.[0-9]+)$')) {
            throw "Unexpected release URL: $latest"
        }
        $tag = $Matches[1]
        $version = $tag.Substring(1)
        $updating = -not [string]::IsNullOrEmpty($env:YINKOTE_UPDATE_TARGET)
        if ($updating) {
            $current = $env:YINKOTE_CURRENT_VERSION
            if ($current -notmatch '^([0-9]+\.[0-9]+\.[0-9]+)([-+][0-9A-Za-z.+-]+)?$') {
                throw 'The updater did not provide a valid current version'
            }
            $baseVersion = [version]$Matches[1]
            if ([version]$version -lt $baseVersion -or
                ([version]$version -eq $baseVersion -and $current -notmatch '^[0-9]+\.[0-9]+\.[0-9]+-')) {
                Write-Host "Yinkote $current is up to date (latest release: $tag)."
                return
            }
            $destination = [IO.Path]::GetFullPath($env:YINKOTE_UPDATE_TARGET)
        } else {
            $directory = $env:YINKOTE_INSTALL_DIR
            if ([string]::IsNullOrEmpty($directory)) {
                if ([string]::IsNullOrEmpty($env:LOCALAPPDATA)) {
                    throw 'LOCALAPPDATA is unset; set YINKOTE_INSTALL_DIR'
                }
                $directory = Join-Path $env:LOCALAPPDATA 'Programs\Yinkote'
            }
            $destination = [IO.Path]::GetFullPath((Join-Path $directory 'yinkote.exe'))
        }
        $directory = [IO.Path]::GetDirectoryName($destination)
        if ([IO.Directory]::Exists($destination)) { throw "Destination is a directory: $destination" }
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        $staging = Join-Path $directory ('.yinkote-install-' + [guid]::NewGuid().ToString('N'))
        [IO.Directory]::CreateDirectory($staging) | Out-Null
        $download = Join-Path $staging 'yinkote.exe'
        $checksumFile = Join-Path $staging 'checksum'
        $asset = 'yinkote-x86_64-pc-windows-msvc.exe'
        Write-Host "Downloading Yinkote $tag (Windows x64)..."
        Invoke-WebRequest -UseBasicParsing -Uri "$repo/releases/download/$tag/$asset" -OutFile $download -UserAgent 'yinkote-installer' -TimeoutSec 300
        Invoke-WebRequest -UseBasicParsing -Uri "$repo/releases/download/$tag/$asset.sha256" -OutFile $checksumFile -UserAgent 'yinkote-installer' -TimeoutSec 300
        $checksum = ([IO.File]::ReadAllText($checksumFile).Trim() -split '\s+')[0]
        if ($checksum -notmatch '^[0-9a-fA-F]{64}$') { throw 'Invalid SHA-256 checksum' }
        if ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash -ne $checksum) {
            throw 'SHA-256 mismatch; the existing installation was not changed'
        }
        $reported = (& $download --version) -join "`n"
        if ($LASTEXITCODE -ne 0 -or $reported.Trim() -ne "yinkote $version") {
            throw 'The verified binary cannot run or reports the wrong version; the existing installation was not changed'
        }

        # Windows can rename an in-use executable, but may not delete it until
        # its last process exits. Keep at most one explicitly named old copy.
        $backup = "$destination.old"
        if ([IO.File]::Exists($backup)) {
            try { [IO.File]::Delete($backup) }
            catch { throw "Cannot remove $backup. Stop the old Yinkote processes before retrying. $($_.Exception.Message)" }
        }
        $hadPrevious = [IO.File]::Exists($destination)
        if ($hadPrevious) { [IO.File]::Move($destination, $backup) }
        try {
            [IO.File]::Move($download, $destination)
        } catch {
            $failure = $_
            if ($hadPrevious) {
                try { [IO.File]::Move($backup, $destination) }
                catch { throw "Installation and rollback failed. The previous executable is at $backup. $($_.Exception.Message)" }
            }
            throw $failure
        }
        if ($hadPrevious) {
            try { [IO.File]::Delete($backup) }
            catch { Write-Warning "Installed successfully, but $backup is still in use or could not be removed. Stop old processes before the next update. $($_.Exception.Message)" }
        }

        if (-not $updating) {
            $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
            $entries = @($userPath -split ';' | Where-Object { $_ })
            if ($entries -notcontains $directory) {
                try { [Environment]::SetEnvironmentVariable('Path', (($entries + $directory) -join ';'), 'User') }
                catch { throw "The binary was installed at $destination, but updating your user PATH failed. Add $directory manually. $($_.Exception.Message)" }
            }
            if (@($env:Path -split ';') -notcontains $directory) {
                $env:Path = ([string]$env:Path).TrimEnd(';') + ';' + $directory
            }
        }
        Write-Host "Installed Yinkote $version to $destination"
        Write-Host 'Library data was not changed. Restart any running Yinkote server to use this version.'
        if (-not $updating) { Write-Host 'New terminals will find yinkote on your user PATH.' }
    } finally {
        [Net.ServicePointManager]::SecurityProtocol = $protocol
        if ($staging -and [IO.Directory]::Exists($staging)) {
            foreach ($name in @('yinkote.exe', 'checksum')) {
                $path = Join-Path $staging $name
                if ([IO.File]::Exists($path)) { [IO.File]::Delete($path) }
            }
            [IO.Directory]::Delete($staging)
        }
    }
}
