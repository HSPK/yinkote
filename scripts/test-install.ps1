param(
    [Parameter(Mandatory = $true)]
    [string]$Binary
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$installer = Join-Path (Split-Path $PSScriptRoot -Parent) 'install.ps1'
$version = ((& $Binary --version) -replace '^yinkote ', '').Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot run the installer fixture executable' }
$root = Join-Path ([IO.Path]::GetTempPath()) ('yinkote-installer-test-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($root) | Out-Null
$originalPath = $env:Path
$originalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$savedEnvironment = @{}
foreach ($name in @('YINKOTE_INSTALL_DIR', 'YINKOTE_UPDATE_TARGET', 'YINKOTE_CURRENT_VERSION', 'PROCESSOR_ARCHITECTURE', 'PROCESSOR_ARCHITEW6432', 'LOCALAPPDATA')) {
    $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
$global:YinkoteInstallerFixture = @{
    Requests = [Collections.Generic.List[string]]::new()
    BadChecksum = $false
    FailDownload = $false
    NewResponseShape = $false
    Version = $version
    Binary = $Binary
}
$server = $null

function Assert-True($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

# Exercise the real installer without GitHub access or global test dependencies.
function Invoke-WebRequest {
    param([switch]$UseBasicParsing, [string]$Uri, [string]$Method,
          [string]$OutFile, [string]$UserAgent, [int]$TimeoutSec)
    $fixture = $global:YinkoteInstallerFixture
    $fixture.Requests.Add($Uri)
    if ($Method -eq 'Head') {
        Assert-True ($Uri -eq 'https://github.com/HSPK/yinkote/releases/latest') 'Wrong latest-release endpoint'
        $resolved = [uri]"https://github.com/HSPK/yinkote/releases/tag/v$($fixture.Version)"
        if ($fixture.NewResponseShape) {
            return [pscustomobject]@{ BaseResponse = [pscustomobject]@{
                RequestMessage = [pscustomobject]@{ RequestUri = $resolved }
            } }
        }
        return [pscustomobject]@{ BaseResponse = [pscustomobject]@{ ResponseUri = $resolved } }
    }
    if ($fixture.FailDownload) { throw 'Simulated download failure' }
    $asset = "https://github.com/HSPK/yinkote/releases/download/v$($fixture.Version)/yinkote-x86_64-pc-windows-msvc.exe"
    Assert-True ($Uri -eq $asset -or $Uri -eq "$asset.sha256") 'An asset was not pinned to the resolved release'
    if ($Uri.EndsWith('.sha256')) {
        $hash = (Get-FileHash -LiteralPath $fixture.Binary -Algorithm SHA256).Hash
        if ($fixture.BadChecksum) { $hash = '0' * 64 }
        [IO.File]::WriteAllText($OutFile, "$hash  yinkote-x86_64-pc-windows-msvc.exe`n")
    } else {
        [IO.File]::Copy($fixture.Binary, $OutFile)
        if ($env:OS -ne 'Windows_NT') {
            & chmod +x $OutFile
            if ($LASTEXITCODE -ne 0) { throw 'Could not make the fixture executable' }
        }
    }
}

function Assert-Fails([scriptblock]$Action, [string]$MessagePart) {
    try { & $Action }
    catch {
        if ($_.Exception.Message -notlike "*$MessagePart*") { throw }
        return
    }
    throw "Expected failure containing: $MessagePart"
}

try {
    $env:PROCESSOR_ARCHITECTURE = 'AMD64'
    $env:PROCESSOR_ARCHITEW6432 = ''
    $env:YINKOTE_UPDATE_TARGET = ''
    $env:YINKOTE_CURRENT_VERSION = ''
    $env:LOCALAPPDATA = Join-Path $root 'local-app-data'
    $env:YINKOTE_INSTALL_DIR = ''
    & $installer
    Assert-True ([IO.File]::Exists((Join-Path $env:LOCALAPPDATA 'Programs\Yinkote\yinkote.exe'))) 'Wrong default installation directory'
    $global:YinkoteInstallerFixture.Requests.Clear()
    $env:YINKOTE_INSTALL_DIR = Join-Path $root "directory with 'quotes' and spaces"
    $destination = Join-Path $env:YINKOTE_INSTALL_DIR 'yinkote.exe'

    & $installer
    Assert-True ([IO.File]::Exists($destination)) 'Fresh installation did not create the executable'
    Assert-True ((& $destination --version) -eq "yinkote $version") 'Wrong installed executable'
    Assert-True ($global:YinkoteInstallerFixture.Requests.Count -eq 3) 'Expected one resolution and two pinned downloads'
    if ($env:OS -eq 'Windows_NT') {
        Assert-True (@([Environment]::GetEnvironmentVariable('Path', 'User') -split ';') -contains $env:YINKOTE_INSTALL_DIR) 'User PATH was not updated'
    }
    Assert-True (@($env:Path -split ';') -contains $env:YINKOTE_INSTALL_DIR) 'Current PATH was not updated'
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash

    $env:YINKOTE_UPDATE_TARGET = $destination
    foreach ($current in @($version, '999.0.0', "$version+build-with-hyphens")) {
        $global:YinkoteInstallerFixture.Requests.Clear()
        $env:YINKOTE_CURRENT_VERSION = $current
        & $installer
        Assert-True ($global:YinkoteInstallerFixture.Requests.Count -eq 1) 'Up-to-date or newer installs downloaded assets'
    }

    $env:YINKOTE_CURRENT_VERSION = '0.0.0'
    $global:YinkoteInstallerFixture.BadChecksum = $true
    Assert-Fails { & $installer } 'SHA-256 mismatch'
    Assert-True ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $hash) 'Bad checksum replaced the executable'
    $global:YinkoteInstallerFixture.BadChecksum = $false
    $global:YinkoteInstallerFixture.FailDownload = $true
    Assert-Fails { & $installer } 'Simulated download failure'
    Assert-True ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $hash) 'Failed download replaced the executable'
    $global:YinkoteInstallerFixture.FailDownload = $false
    $global:YinkoteInstallerFixture.Version = '999.0.0'
    Assert-Fails { & $installer } 'reports the wrong version'
    Assert-True ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $hash) 'Wrong binary version replaced the executable'
    $global:YinkoteInstallerFixture.Version = $version

    $env:PROCESSOR_ARCHITEW6432 = 'ARM64'
    Assert-Fails { & $installer } 'Only Windows x64'
    $env:PROCESSOR_ARCHITEW6432 = ''
    $global:YinkoteInstallerFixture.NewResponseShape = $true
    $env:YINKOTE_CURRENT_VERSION = "$version-rc.1"
    & $installer
    Assert-True ((& $destination --version) -eq "yinkote $version") 'Stable release did not replace a prerelease'

    if ($env:OS -eq 'Windows_NT') {
        $info = [Diagnostics.ProcessStartInfo]::new()
        $info.FileName = $destination
        $info.Arguments = '--host 127.0.0.1 --port 0 --data-dir "' + (Join-Path $root 'library') + '"'
        $info.UseShellExecute = $false
        $info.CreateNoWindow = $true
        $server = [Diagnostics.Process]::Start($info)
        Start-Sleep -Milliseconds 1500
        Assert-True (-not $server.HasExited) 'The in-use executable fixture exited'
        $env:YINKOTE_CURRENT_VERSION = '0.0.0'
        & $installer
        Assert-True (-not $server.HasExited) 'Updating unexpectedly stopped the server'
        Assert-True ([IO.File]::Exists("$destination.old")) 'An in-use old executable was not retained'
        Assert-True ((& $destination --version) -eq "yinkote $version") 'New executable is not runnable'
        Assert-Fails { & $installer } 'Stop the old Yinkote processes'
        $server.Kill()
        $server.WaitForExit()
        $server = $null
        & $installer
        Assert-True (-not [IO.File]::Exists("$destination.old")) 'Stopped executable backup was not cleaned up'
    }
    Assert-True (@(Get-ChildItem -LiteralPath $env:YINKOTE_INSTALL_DIR -Force | Where-Object { $_.Name -like '.yinkote-install-*' }).Count -eq 0) 'Installer leaked staging directories'
    Write-Host 'PowerShell installer checks passed.'
} finally {
    if ($null -ne $server -and -not $server.HasExited) {
        $server.Kill()
        $server.WaitForExit()
    }
    [Environment]::SetEnvironmentVariable('Path', $originalUserPath, 'User')
    $env:Path = $originalPath
    foreach ($name in $savedEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process')
    }
    Remove-Item -LiteralPath $root -Recurse -Force
    Remove-Variable -Name YinkoteInstallerFixture -Scope Global
}
