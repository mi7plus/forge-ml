# Stage Forge's sibling helper binaries (and the CEF/Chromium runtime the web
# preview needs) into packaging/helpers so `cargo packager` bundles them beside
# the main exe. Run before packaging, like build-offline-bundle.ps1.
#
# Windows layout produced (installed next to forge_ide.exe as helpers/):
#   helpers/forge_cef.exe        offscreen-render helper (Chromium host)
#   helpers/forge_manager.exe    environment manager GUI
#   helpers/forge_webview.exe    WebView2 preview window
#   helpers/libcef.dll + *.pak + icudtl.dat + locales/ + …   CEF runtime
# forge_cef.exe loads libcef.dll from its own dir, so the runtime sits with it.

#Requires -Version 5
$ErrorActionPreference = "Stop"

$scriptDir = $PSScriptRoot
$root = Split-Path -Parent $scriptDir
$helpers = Join-Path $scriptDir "helpers"

Write-Host "Staging Forge helpers into $helpers"
if (Test-Path $helpers) { Remove-Item -Recurse -Force $helpers }
New-Item -ItemType Directory -Force -Path $helpers | Out-Null

# Ensure CMake + Ninja are available for cef-dll-sys's C++ wrapper build.
if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) {
    Write-Host "Ninja not on PATH; installing via choco."
    choco install ninja -y | Out-Null
}

Push-Location $root
try {
    cargo build --release -p forge-cef -p forge-manager -p forge-webview
    if ($LASTEXITCODE -ne 0) { throw "cargo build of helpers failed" }
} finally {
    Pop-Location
}

$rel = Join-Path $root "target/release"
foreach ($bin in @("forge_cef.exe", "forge_manager.exe", "forge_webview.exe")) {
    Copy-Item (Join-Path $rel $bin) (Join-Path $helpers $bin) -Force
}

# Locate the CEF distribution cef-dll-sys downloaded during the build and copy
# the files forge_cef.exe needs to load Chromium at run time (not the SDK's
# headers/libs/cmake, which are build-time only).
$cefDir = Get-ChildItem -Path (Join-Path $rel "build") -Recurse -Directory -Filter "cef_windows_x86_64" -ErrorAction SilentlyContinue |
    Where-Object { Test-Path (Join-Path $_.FullName "libcef.dll") } |
    Select-Object -First 1 -ExpandProperty FullName
if (-not $cefDir) {
    throw "CEF runtime not found under $rel/build (expected cef_windows_x86_64/libcef.dll)"
}
Write-Host "CEF runtime source: $cefDir"

$runtimeFiles = @(
    "libcef.dll", "chrome_elf.dll", "libEGL.dll", "libGLESv2.dll", "d3dcompiler_47.dll",
    "dxcompiler.dll", "dxil.dll", "vk_swiftshader.dll", "vulkan-1.dll",
    "icudtl.dat", "chrome_100_percent.pak", "chrome_200_percent.pak", "resources.pak",
    "v8_context_snapshot.bin", "vk_swiftshader_icd.json"
)
foreach ($f in $runtimeFiles) {
    $src = Join-Path $cefDir $f
    if (Test-Path $src) {
        Copy-Item $src (Join-Path $helpers $f) -Force
    } else {
        Write-Warning "CEF runtime file missing (skipped): $f"
    }
}
$locales = Join-Path $cefDir "locales"
if (Test-Path $locales) {
    Copy-Item $locales (Join-Path $helpers "locales") -Recurse -Force
}

Write-Host "Staged helpers:"
Get-ChildItem $helpers | Select-Object Name, Length | Format-Table -AutoSize
