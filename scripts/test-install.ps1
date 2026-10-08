param(
    [Parameter(Mandatory = $true)]
    [string]$Binary,
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target = 'x86_64-pc-windows-msvc'
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$installer = Join-Path (Split-Path $PSScriptRoot -Parent) 'install.ps1'
$version = ((& $Binary --version) -replace '^yinkote ', '').Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot run the installer fixture executable' }
$nativeArchitecture = 9
if ($Target -eq 'aarch64-pc-windows-msvc') { $nativeArchitecture = 12 }
if ($env:OS -eq 'Windows_NT') {
    $stream = [IO.File]::OpenRead($Binary)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($reader.ReadUInt16() -ne 0x5a4d) { throw 'Fixture is not a Windows executable' }
        $stream.Position = 0x3c
        $stream.Position = $reader.ReadUInt32()
        if ($reader.ReadUInt32() -ne 0x00004550) { throw 'Fixture has no PE signature' }
        $expectedMachine = 0x8664
        if ($Target -eq 'aarch64-pc-windows-msvc') { $expectedMachine = 0xaa64 }
        if ($reader.ReadUInt16() -ne $expectedMachine) { throw 'Fixture was built for the wrong architecture' }
    } finally {
        $reader.Dispose()
    }
}
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
    Target = $Target
    Architecture = $nativeArchitecture
    AddressWidth = 64
    NativeQuery = ($env:OS -eq 'Windows_NT')
    FailQuery = $false
    EmptyQuery = $false
}
$server = $null

function Assert-True($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Get-CimInstance {
    [CmdletBinding()]
    param([string]$ClassName, [string[]]$Property)
    $fixture = $global:YinkoteInstallerFixture
    Assert-True ($ClassName -eq 'Win32_Processor') 'Wrong platform query'
    Assert-True ($Property -contains 'Architecture' -and $Property -contains 'AddressWidth') 'Platform query omitted CPU or OS width'
    if ($fixture.NativeQuery) {
        return CimCmdlets\Get-CimInstance @PSBoundParameters
    }
    if ($fixture.FailQuery) { throw 'Simulated platform detection failure' }
    if ($fixture.EmptyQuery) { return }
    return [pscustomobject]@{ Architecture = $fixture.Architecture; AddressWidth = $fixture.AddressWidth }
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
    $assetName = "yinkote-$($fixture.Target).exe"
    $asset = "https://github.com/HSPK/yinkote/releases/download/v$($fixture.Version)/$assetName"
    Assert-True ($Uri -eq $asset -or $Uri -eq "$asset.sha256") 'An asset was not pinned to the resolved release'
    if ($Uri.EndsWith('.sha256')) {
        $hash = (Get-FileHash -LiteralPath $fixture.Binary -Algorithm SHA256).Hash
        if ($fixture.BadChecksum) { $hash = '0' * 64 }
        [IO.File]::WriteAllText($OutFile, "$hash  $assetName`n")
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

    # Native and emulated shells must all follow the platform, not the guest
    # environment. Downloads remain mocked; native CI also runs a real PE.
    $global:YinkoteInstallerFixture.NativeQuery = $false
    foreach ($case in @(
        @{ Cpu = 12; Process = 'ARM64'; Wow = ''; Target = 'aarch64-pc-windows-msvc' },
        @{ Cpu = 12; Process = 'AMD64'; Wow = ''; Target = 'aarch64-pc-windows-msvc' },
        @{ Cpu = 12; Process = 'x86'; Wow = 'ARM64'; Target = 'aarch64-pc-windows-msvc' },
        @{ Cpu = 9; Process = 'AMD64'; Wow = ''; Target = 'x86_64-pc-windows-msvc' },
        @{ Cpu = 9; Process = 'x86'; Wow = 'AMD64'; Target = 'x86_64-pc-windows-msvc' }
    )) {
        $global:YinkoteInstallerFixture.Architecture = $case.Cpu
        $global:YinkoteInstallerFixture.Target = $case.Target
        $env:PROCESSOR_ARCHITECTURE = $case.Process
        $env:PROCESSOR_ARCHITEW6432 = $case.Wow
        $global:YinkoteInstallerFixture.Requests.Clear()
        & $installer
        Assert-True ($global:YinkoteInstallerFixture.Requests.Count -eq 3) 'Architecture selection did not download one pinned asset pair'
    }
    foreach ($unsupported in @(0, 5, 6, 99)) {
        $global:YinkoteInstallerFixture.Architecture = $unsupported
        $global:YinkoteInstallerFixture.Requests.Clear()
        Assert-Fails { & $installer } 'Unsupported Windows processor architecture'
        Assert-True ($global:YinkoteInstallerFixture.Requests.Count -eq 0) 'Unsupported platform made a network request'
    }
    $global:YinkoteInstallerFixture.Architecture = $nativeArchitecture
    $global:YinkoteInstallerFixture.Target = $Target
    $global:YinkoteInstallerFixture.AddressWidth = 32
    Assert-Fails { & $installer } 'supported 64-bit Windows platform'
    $global:YinkoteInstallerFixture.AddressWidth = 64
    $global:YinkoteInstallerFixture.EmptyQuery = $true
    Assert-Fails { & $installer } 'supported 64-bit Windows platform'
    $global:YinkoteInstallerFixture.EmptyQuery = $false
    $global:YinkoteInstallerFixture.FailQuery = $true
    Assert-Fails { & $installer } 'Simulated platform detection failure'
    $global:YinkoteInstallerFixture.FailQuery = $false
    $global:YinkoteInstallerFixture.NativeQuery = ($env:OS -eq 'Windows_NT')
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
