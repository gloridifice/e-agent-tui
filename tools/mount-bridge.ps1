# mount-bridge.ps1 — link dsh-tui-bridge into the DSH web profile.
# Idempotent: safe to rerun. Requires write access to $DSH_HOME.
param([string]$DshHome = $env:DSH_HOME)

$ErrorActionPreference = 'Stop'
if (-not $DshHome) { throw 'DSH_HOME is empty' }
$profile = Join-Path $DshHome 'profiles\web'
if (-not (Test-Path $profile)) { throw "profile not found: $profile" }

$packagesDir = Join-Path $profile 'packages'
$bridgeDir = Join-Path $packagesDir 'dsh-tui-bridge'
$bridgeSrc = (Resolve-Path (Join-Path $PSScriptRoot '..\bridge')).Path

# 1. mirror bridge/ into the profile (a junction breaks node module resolution:
#    the package's physical path must sit under the profile's node_modules tree).
New-Item -ItemType Directory -Path $packagesDir -Force | Out-Null
if (Test-Path $bridgeDir) {
    cmd /c rmdir /s /q "$bridgeDir" | Out-Null
    Write-Host '[1/4] previous install removed (junction -> mirror copy)'
}
robocopy $bridgeSrc $bridgeDir /MIR /NFL /NDL /NJH /NJS /NP | Out-Null
Write-Host '[1/4] bridge mirrored into profile'

# 2. pnpm-workspace.yaml: add packages/*
$wsFile = Join-Path $profile 'pnpm-workspace.yaml'
$ws = Get-Content $wsFile -Raw
if ($ws -match 'packages/\*') {
    Write-Host '[2/4] workspace already lists packages/*'
} else {
    $ws = $ws -replace 'packages:\r?\n\s+- \.', "packages:`n  - .`n  - packages/*"
    Set-Content -Path $wsFile -Value $ws -Encoding utf8
    Write-Host '[2/4] workspace updated'
}

# 3. package.json: dependency on the local package (via node for stable JSON)
$pjFile = Join-Path $profile 'package.json'
$depAdded = node -e @"
const fs = require('fs')
const p = process.argv[1]
const j = JSON.parse(fs.readFileSync(p, 'utf8'))
if (!j.dependencies) j.dependencies = {}
if (!j.dependencies['dsh-tui-bridge']) {
  j.dependencies['dsh-tui-bridge'] = 'workspace:*'
  fs.writeFileSync(p, JSON.stringify(j, null, 2) + '\n')
  console.log('added')
} else {
  console.log('exists')
}
"@ -- $pjFile
Write-Host "[3/4] package.json dependency: $depAdded"

# 4. cordis.patch.yml: insert the bridge row at the top level.
#    The file is a top-level array; replace the whole document (keeping the
#    header comment) instead of appending after a legacy `[]` line.
$patchFile = Join-Path $profile 'cordis.patch.yml'
$patch = Get-Content $patchFile -Raw
if ($patch -match 'tui-bridge') {
    Write-Host '[4/4] patch already registers tui-bridge'
} else {
    $header = ($patch -split "`n" | Where-Object { $_.TrimStart().StartsWith('#') }) -join "`n"
    $patch = $header + "`n" + @"
- insert:
    - id: tui-bridge
      name: dsh-tui-bridge
"@
    Set-Content -Path $patchFile -Value $patch -Encoding utf8
    Write-Host '[4/4] cordis.patch.yml updated'
}

Write-Host ''
Write-Host 'Next:  dsh plugin --profile web install'
Write-Host 'Then:  dsh --profile web --dump-config | Select-String tui-bridge'
Write-Host 'Then:  restart `dsh web` to load the bridge'
