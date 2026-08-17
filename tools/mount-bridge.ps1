# mount-bridge.ps1 — link dsh-tui-bridge into a DSH profile, creating the
# profile skeleton first when it does not exist (used for both the `web`
# profile and the `dshe` launcher's dedicated `dshe` profile).
#
# Idempotent: safe to rerun. Requires write access to $DSH_HOME.
param(
    [string]$DshHome = $env:DSH_HOME,
    [string]$Profile = 'dshe'
)

$ErrorActionPreference = 'Stop'

# DSH itself defaults to ~/.dsh. Mirror that behavior so a fresh shell does
# not require callers to create DSH_HOME before mounting the bridge.
if ([string]::IsNullOrWhiteSpace($DshHome)) {
    $userHome = [Environment]::GetFolderPath('UserProfile')
    if ([string]::IsNullOrWhiteSpace($userHome)) { $userHome = $HOME }
    if ([string]::IsNullOrWhiteSpace($userHome)) {
        throw 'Cannot resolve the user home; set DSH_HOME or pass -DshHome explicitly'
    }
    $DshHome = Join-Path $userHome '.dsh'
    Write-Host "DSH_HOME is not set; using $DshHome"
}

# PowerShell variable names are case-insensitive: keep the profile name in the
# parameter `$Profile` and use a distinct name for its directory.
$profileDir = Join-Path $DshHome "profiles\$Profile"
$packagesDir = Join-Path $profileDir 'packages'
$bridgeDir = Join-Path $packagesDir 'dsh-tui-bridge'
$bridgeSrc = (Resolve-Path (Join-Path $PSScriptRoot '..\bridge')).Path
$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# ---- 0. create the profile skeleton when it does not exist ---------------
if (-not (Test-Path (Join-Path $profileDir 'package.json'))) {
    New-Item -ItemType Directory -Path $profileDir -Force | Out-Null
    $pkgJson = @{
        name = "dsh-profile-$Profile"
        private = $true
        dependencies = @{ 'dsh-win32' = '0.11.2' }
        dsh = @{ profile = @{ bundles = @('@deepseek-ai/dsh-base', '@deepseek-ai/dsh-web-app', 'dsh-win32') } }
    } | ConvertTo-Json -Depth 5
    # Windows PowerShell 5.1's `Set-Content -Encoding utf8` emits a BOM, which
    # makes Node's JSON.parse reject a freshly-created package.json.
    [System.IO.File]::WriteAllText((Join-Path $profileDir 'package.json'), "$pkgJson`r`n", $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $profileDir 'pnpm-workspace.yaml'), "packages:`n  - .`n  - packages/*`n", $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $profileDir 'cordis.yml'), "[]`n", $utf8NoBom)
    $initialPatch = @'
# Your patch layer for this dsh profile, applied after every bundle layer:
# a top-level YAML array of loader patch entries.
'@
    [System.IO.File]::WriteAllText((Join-Path $profileDir 'cordis.patch.yml'), "$initialPatch`n", $utf8NoBom)
    Write-Host "[0/4] profile $Profile created (bundles: base + web-app + win32)"
} else {
    Write-Host "[0/4] profile $Profile already exists"
}

# ---- 1. mirror bridge/ into the profile (a junction breaks node module
#         resolution: the package must sit under the profile's node_modules
#         tree, so use a physical copy) ------------------------------------
New-Item -ItemType Directory -Path $packagesDir -Force | Out-Null
if (Test-Path $bridgeDir) {
    cmd /c rmdir /s /q "$bridgeDir" | Out-Null
}
robocopy $bridgeSrc $bridgeDir /MIR /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "robocopy failed with exit code $LASTEXITCODE" }
Write-Host '[1/4] bridge mirrored into profile'

# ---- 2. pnpm-workspace.yaml: add packages/* ------------------------------
$wsFile = Join-Path $profileDir 'pnpm-workspace.yaml'
$ws = Get-Content $wsFile -Raw
if ($ws -match 'packages/\*') {
    Write-Host '[2/4] workspace already lists packages/*'
} else {
    $ws = $ws -replace 'packages:\r?\n\s+- \.', "packages:`n  - .`n  - packages/*"
    [System.IO.File]::WriteAllText($wsFile, $ws, $utf8NoBom)
    Write-Host '[2/4] workspace updated'
}

# ---- 3. package.json: dependency on the local package --------------------
$pjFile = Join-Path $profileDir 'package.json'
$depAdded = node -e @"
const fs = require('fs')
const p = process.argv[1]
const j = JSON.parse(fs.readFileSync(p, 'utf8').replace(/^\uFEFF/, ''))
if (!j.dependencies) j.dependencies = {}
const added = !j.dependencies['dsh-tui-bridge']
if (added) j.dependencies['dsh-tui-bridge'] = 'workspace:*'
// Always rewrite: this also repairs package.json files created with a UTF-8 BOM.
fs.writeFileSync(p, JSON.stringify(j, null, 2) + '\n')
console.log(added ? 'added' : 'exists')
"@ -- $pjFile
if ($LASTEXITCODE -ne 0) { throw "failed to update $pjFile (node exit code $LASTEXITCODE)" }
Write-Host "[3/4] package.json dependency: $depAdded"

# ---- 4. cordis.patch.yml: insert the bridge row --------------------------
$patchFile = Join-Path $profileDir 'cordis.patch.yml'
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
    [System.IO.File]::WriteAllText($patchFile, "$patch`n", $utf8NoBom)
    Write-Host '[4/4] cordis.patch.yml updated'
}

Write-Host ''
Write-Host "Next:  dsh plugin --profile $Profile install"
Write-Host "Then:  dsh --profile $Profile --dump-config | Select-String tui-bridge"
Write-Host "Then:  run dshe (spawns this profile automatically) or dsh --profile $Profile"
