# build-pkg.ps1
# Builds the WASM package and patches pkg/ for Vite / ESM consumers.
# Usage: .\build-pkg.ps1

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root     = $PSScriptRoot
$crate    = "$root\crates\web_binding"
$pkg      = "$crate\pkg"
$srcTypes = "$crate\types\index.d.ts"
$srcLic   = "$root\..\LICENSE"
$pkgJsonPath = "$pkg\package.json"

$previousVersion = $null
if (Test-Path $pkgJsonPath) {
    try {
        $previousVersion = (Get-Content $pkgJsonPath -Raw | ConvertFrom-Json).version
    } catch {
        $previousVersion = $null
    }
}

if (Test-Path $pkg) {
    Write-Host "==> Removing stale pkg/" -ForegroundColor Cyan
    Remove-Item $pkg -Recurse -Force
}

Write-Host "==> wasm-pack build (bundler target)" -ForegroundColor Cyan
wasm-pack build $crate --target bundler --out-dir pkg
if ($LASTEXITCODE -ne 0) { throw "wasm-pack failed" }

Write-Host "==> Patching pkg/package.json" -ForegroundColor Cyan
$generatedVersion = $null
if (Test-Path $pkgJsonPath) {
    try {
        $generatedVersion = (Get-Content $pkgJsonPath -Raw | ConvertFrom-Json).version
    } catch {
        $generatedVersion = $null
    }
}

$effectiveVersion = $previousVersion
if (-not $effectiveVersion) { $effectiveVersion = $generatedVersion }
if (-not $effectiveVersion) { $effectiveVersion = "0.1.0" }

$json = [ordered]@{
    name        = "ipa-poetry-engine"
    type        = "module"
    version     = $effectiveVersion
    description = "IPA poetry analysis engine - WebAssembly / npm binding"
    author      = "Tilitronic"
    license     = "AGPL-3.0-or-later"
    repository  = [ordered]@{
        type = "git"
        url  = "https://github.com/Tilitronic/ipa-poetry-engine.git"
    }
    files       = @(
        "ipa_poetry_engine_bg.wasm"
        "ipa_poetry_engine_bg.js"
        "ipa_poetry_engine.js"
        "ipa_poetry_engine.d.ts"
        "ipa_poetry_engine_bg.wasm.d.ts"
        "types.d.ts"
        "LICENSE"
    )
    main        = "ipa_poetry_engine.js"
    types       = "types.d.ts"
    exports     = [ordered]@{
        "." = [ordered]@{
            types  = "./types.d.ts"
            import = "./ipa_poetry_engine.js"
            default = "./ipa_poetry_engine.js"
        }
        "./wasm" = "./ipa_poetry_engine_bg.wasm"
    }
    # Tells bundlers the JS init file has side effects (WASM init),
    # preventing incorrect tree-shaking.
    sideEffects = @("./ipa_poetry_engine.js")
}
$json | ConvertTo-Json -Depth 6 | Set-Content "$pkg\package.json" -Encoding UTF8

Write-Host "==> Copying types.d.ts" -ForegroundColor Cyan
Copy-Item $srcTypes "$pkg\types.d.ts" -Force

Write-Host "==> Copying LICENSE" -ForegroundColor Cyan
Copy-Item $srcLic "$pkg\LICENSE" -Force

Write-Host ""
Write-Host "Done. pkg/ contents:" -ForegroundColor Green
Get-ChildItem $pkg | Select-Object Name, @{N="KB";E={[math]::Round($_.Length/1KB,1)}} | Format-Table -AutoSize
