#Requires -Version 5.1
<#
.SYNOPSIS
  Native Windows build of Tephra (MSVC toolchain), for a clone of the repo on Windows.

.DESCRIPTION
  Builds target\release\tephra.exe and packs dist\tephra-<version>-windows-x86_64.zip.
  Run from anywhere:
      powershell -ExecutionPolicy Bypass -File packaging\windows\build.ps1
  (or `make windows` if you have GNU make on Windows).

  Prerequisites (one-time; the winget commands install them):
    - Rust via rustup                 winget install Rustlang.Rustup
      (rust-toolchain.toml pins the version; rustup installs it on the first build)
    - Visual Studio Build Tools with the "Desktop development with C++" workload
      (MSVC compiler + linker and the Windows SDK, which provides fxc.exe for GPUI's DirectX
      shaders and rc.exe for the exe icon)
                                      winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
    - Node.js, only if the JS bundles (crates\app\assets\*.js, gitignored) are not built yet
                                      winget install OpenJS.NodeJS.LTS

.PARAMETER NoZip
  Only build the exe, skip the zip.
#>
param([switch]$NoZip)

$ErrorActionPreference = 'Stop'
Set-Location (Resolve-Path (Join-Path $PSScriptRoot '..\..'))

function Require($cmd, $hint) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) { throw "'$cmd' not found. $hint" }
}
function Run {
    # no declared parameters, so flags like -p / --locked pass through to the command untouched
    $cmd, $rest = $args
    & $cmd @rest
    if ($LASTEXITCODE -ne 0) { throw "$args failed (exit code $LASTEXITCODE)" }
}

# --- prerequisites -------------------------------------------------------------------------------
Require cargo 'Install Rust: winget install Rustlang.Rustup  (then open a new terminal)'

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vs = if (Test-Path $vswhere) {
    & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
}
if (-not $vs) {
    throw 'MSVC build tools not found. Install the "Desktop development with C++" workload: ' +
          'winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"'
}

# GPUI's build script compiles its HLSL shaders with the Windows SDK's fxc.exe (it finds it itself;
# this is only an early, clearer error). GPUI_FXC_PATH overrides the location.
if (-not $env:GPUI_FXC_PATH) {
    $fxc = Get-ChildItem (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin\*\x64\fxc.exe') -ErrorAction SilentlyContinue
    if (-not $fxc) {
        throw 'fxc.exe (Windows 10/11 SDK) not found. Add the Windows SDK to the Build Tools install ' +
              '(Visual Studio Installer > Modify > "Desktop development with C++" includes it), or set GPUI_FXC_PATH.'
    }
}

# --- JS bundles (embedded with include_str!, gitignored) ------------------------------------------
$assets = 'crates\app\assets'
if (-not (Test-Path "$assets\mathjax-svg.js") -or -not (Test-Path "$assets\cm6-headless.js")) {
    Require npm 'The JS bundles need Node.js once: winget install OpenJS.NodeJS.LTS'
    Write-Host '==> building JS bundles (tools\js-bundles)'
    Run npm ci --prefix tools/js-bundles --no-audit --no-fund
    Run npm run build --prefix tools/js-bundles
}

# --- build -----------------------------------------------------------------------------------------
Write-Host '==> cargo build --release -p tephra'
Run cargo build --locked --release -p tephra
$exe = 'target\release\tephra.exe'
Write-Host "built $exe"
if ($NoZip) { return }

# --- zip -------------------------------------------------------------------------------------------
$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$name = "tephra-$version-windows-x86_64"
$stage = "target\pkg\windows\$name"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Force $stage, dist | Out-Null
Copy-Item $exe $stage
Copy-Item LICENSE "$stage\LICENSE.txt"
Copy-Item "$assets\fonts\LICENSE-IBMPlexSans.txt", "$assets\fonts\LICENSE-DejaVu.txt" $stage
Copy-Item packaging\windows\README.txt $stage
$zip = "dist\$name.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path $stage -DestinationPath $zip
Write-Host "packed $zip"
