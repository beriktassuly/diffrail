[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $ExecutablePath,

    [Parameter(Mandatory = $true)]
    [string] $ExpectedVersion
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$PSNativeCommandUseErrorActionPreference = $false

$executable = (Resolve-Path -LiteralPath $ExecutablePath).Path
if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    throw "Executable is not a file: $executable"
}
Get-Command git -ErrorAction Stop | Out-Null

$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("diffrail-install-" + [guid]::NewGuid().ToString('N'))
$fixtureRepository = Join-Path $fixtureRoot 'repository'
$fixtureHooks = Join-Path $fixtureRoot 'empty-hooks'
New-Item -ItemType Directory -Path $fixtureRepository, $fixtureHooks | Out-Null

function Invoke-FixtureGit {
    param([string[]] $CommandArguments)

    $gitArguments = @(
        '-C', $fixtureRepository,
        '-c', 'user.name=Test User',
        '-c', 'user.email=test@example.com',
        '-c', 'commit.gpgsign=false',
        '-c', 'core.autocrlf=false',
        '-c', "core.hooksPath=$fixtureHooks"
    ) + $CommandArguments
    $result = & git @gitArguments 2>&1
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        throw "Fixture Git command failed ($exitCode): $($result -join [Environment]::NewLine)"
    }
    return $result
}

function Invoke-DiffRail {
    param(
        [string[]] $CommandArguments,
        [int] $ExpectedExit = 0
    )

    $result = & $executable @CommandArguments 2>&1
    $exitCode = $LASTEXITCODE
    $output = ($result | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
    if ($exitCode -ne $ExpectedExit) {
        throw "DiffRail command '$($CommandArguments -join ' ')' returned $exitCode; expected ${ExpectedExit}: $output"
    }
    return $output
}

function Write-FixtureFile {
    param(
        [string] $RelativePath,
        [string] $Contents
    )

    $filePath = Join-Path $fixtureRepository $RelativePath
    $parentDirectory = Split-Path -Parent $filePath
    if (-not (Test-Path -LiteralPath $parentDirectory -PathType Container)) {
        New-Item -ItemType Directory -Path $parentDirectory | Out-Null
    }
    [System.IO.File]::WriteAllText($filePath, $Contents, [System.Text.UTF8Encoding]::new($false))
}

Write-Host "Testing installed executable: $executable"
Write-Host "Isolated fixture: $fixtureRepository"

$version = Invoke-DiffRail -CommandArguments @('--version')
if ($version.Trim() -ne "diffrail $ExpectedVersion") {
    throw "Unexpected installed version: $version"
}

Invoke-FixtureGit -CommandArguments @('init', '-q') | Out-Null
$policy = @'
version: 1
policy:
  protected:
    - ".github/**"
tasks:
  app:
    allow:
      - "src/**"
    allow_shared: []
    allow_protected: []
'@
Write-FixtureFile -RelativePath '.diffrail.yml' -Contents ($policy + [Environment]::NewLine)
Write-FixtureFile -RelativePath 'src/main.rs' -Contents ('fn main() {}' + [Environment]::NewLine)
Invoke-FixtureGit -CommandArguments @('add', '--all') | Out-Null
Invoke-FixtureGit -CommandArguments @('commit', '-q', '-m', 'seed') | Out-Null
$trustedBase = (Invoke-FixtureGit -CommandArguments @('rev-parse', 'HEAD')).ToString().Trim()

$validation = Invoke-DiffRail -CommandArguments @('--repo', $fixtureRepository, 'validate')
if (-not $validation.Contains('Valid policy:')) {
    throw "Validation did not report a valid policy: $validation"
}

Write-FixtureFile -RelativePath 'src/main.rs' -Contents ('fn main() { println!("allowed"); }' + [Environment]::NewLine)
$allowed = Invoke-DiffRail -CommandArguments @('--repo', $fixtureRepository, 'check', '--task', 'app', '--base', $trustedBase)
if (-not $allowed.Contains('PASS app')) {
    throw "Scoped change did not report success: $allowed"
}

Write-FixtureFile -RelativePath 'outside.txt' -Contents ('outside assigned scope' + [Environment]::NewLine)
$blocked = Invoke-DiffRail -CommandArguments @('--repo', $fixtureRepository, 'check', '--task', 'app', '--base', $trustedBase) -ExpectedExit 1
if (-not ($blocked.Contains('outside_scope') -and $blocked.Contains('outside.txt'))) {
    throw "Out-of-scope change was not identified: $blocked"
}

$widenedPolicy = $policy.Replace('      - "src/**"', '      - "**"').Replace('allow_protected: []', 'allow_protected: ["**"]')
Write-FixtureFile -RelativePath '.diffrail.yml' -Contents ($widenedPolicy + [Environment]::NewLine)
Write-FixtureFile -RelativePath '.github/workflows/changed.yml' -Contents ('name: changed' + [Environment]::NewLine)
Invoke-FixtureGit -CommandArguments @('add', '--all') | Out-Null
Invoke-FixtureGit -CommandArguments @('commit', '-q', '-m', 'widen policy') | Out-Null
$tampering = Invoke-DiffRail -CommandArguments @('--repo', $fixtureRepository, 'check', '--task', 'app', '--base', $trustedBase) -ExpectedExit 1
foreach ($expectedFinding in @('.diffrail.yml', '.github/workflows/changed.yml', 'outside.txt', 'protected_path')) {
    if (-not $tampering.Contains($expectedFinding)) {
        throw "Trusted-base check missed '$expectedFinding': $tampering"
    }
}

Write-Host "PASS: DiffRail $ExpectedVersion installation, validation, scoped changes, violations, and trusted-base policy protection."
Write-Host "Fixture retained for inspection: $fixtureRepository"

# Expected violation checks return 1; a successful smoke script must return 0.
$global:LASTEXITCODE = 0
