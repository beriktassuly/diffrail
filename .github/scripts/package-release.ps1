#Requires -Version 7.2
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('Validate', 'Binary', 'Plugin', 'Checksums')]
    [string] $Mode,

    [string] $SourceRoot = (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent),
    [string] $OutputDirectory,
    [string] $ReleaseTag,

    [ValidateSet('x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'x86_64-apple-darwin', 'aarch64-apple-darwin')]
    [string] $Target,

    [string] $BinaryPath,
    [switch] $IncludeCrate,
    [switch] $RequireComplete
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$PSNativeCommandUseErrorActionPreference = $false

function Invoke-Checked {
    param([string] $Command, [string[]] $Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed with exit code $LASTEXITCODE."
    }
}

function Write-Utf8 {
    param([string] $Path, [string] $Content)
    [IO.File]::WriteAllText($Path, $Content.Replace("`r`n", "`n"), [Text.UTF8Encoding]::new($false))
}

function New-StagingDirectory {
    $path = Join-Path $OutputDirectory ('.staging-' + [Guid]::NewGuid().ToString('N'))
    return [IO.Directory]::CreateDirectory($path).FullName
}

function New-PortableZip {
    param([string] $ArchivePath, [string] $Root, [string[]] $RelativeFiles)
    if (Test-Path -LiteralPath $ArchivePath) {
        throw "Refusing to overwrite an existing archive: $ArchivePath"
    }
    $archive = [IO.Compression.ZipFile]::Open($ArchivePath, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($relative in ($RelativeFiles | Sort-Object -CaseSensitive)) {
            $entryName = $relative.Replace('\', '/')
            $file = Join-Path $Root $relative
            $entry = $archive.CreateEntry($entryName, [IO.Compression.CompressionLevel]::Optimal)
            $entry.LastWriteTime = [DateTimeOffset]::new(2000, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
            $inputStream = [IO.File]::OpenRead($file)
            $outputStream = $entry.Open()
            try { $inputStream.CopyTo($outputStream) }
            finally {
                $outputStream.Dispose()
                $inputStream.Dispose()
            }
        }
    }
    finally { $archive.Dispose() }
}

$SourceRoot = (Resolve-Path -LiteralPath $SourceRoot).Path
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path $SourceRoot 'target/release-assets'
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null

Push-Location -LiteralPath $SourceRoot
try {
    $metadataJson = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata validation failed.' }
    $metadata = $metadataJson | ConvertFrom-Json
    $package = @($metadata.packages | Where-Object name -eq 'diffrail')
    if ($package.Count -ne 1) { throw 'Expected exactly one diffrail Cargo package.' }
    $version = [string] $package[0].version
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "Unsupported release version: $version" }
    $expectedTag = "v$version"
    if ($ReleaseTag -and $ReleaseTag -cne $expectedTag) {
        throw "Release tag $ReleaseTag does not match Cargo version $expectedTag."
    }

    foreach ($relative in @('plugin.json', '.claude-plugin/plugin.json', '.cursor-plugin/plugin.json')) {
        $manifest = Get-Content -LiteralPath (Join-Path $SourceRoot $relative) -Raw | ConvertFrom-Json
        if ($manifest.name -cne 'diffrail' -or $manifest.version -cne $version) {
            throw "$relative must declare diffrail version $version."
        }
    }
    $skillPath = Join-Path $SourceRoot 'skills/diffrail/SKILL.md'
    if (-not (Test-Path -LiteralPath $skillPath -PathType Leaf)) { throw 'DiffRail skill is missing.' }

    if ($Mode -eq 'Validate') {
        $notesPath = Join-Path $SourceRoot "docs/releases/$expectedTag.md"
        if (-not (Test-Path -LiteralPath $notesPath -PathType Leaf)) {
            throw "Release notes are missing: $notesPath"
        }
        Write-Output "Validated DiffRail $version ($expectedTag)."
    }

    if ($Mode -eq 'Plugin') {
        $relativeFiles = [Collections.Generic.List[string]]::new()
        foreach ($relative in @(
            'plugin.json', '.claude-plugin/plugin.json', '.claude-plugin/marketplace.json',
            '.cursor-plugin/plugin.json', '.agents/plugins/marketplace.json',
            'README.md', 'LICENSE', 'examples/policy.yml'
        )) {
            $relativeFiles.Add($relative)
        }
        foreach ($file in Get-ChildItem -LiteralPath (Join-Path $SourceRoot 'docs') -Filter '*.md' -Recurse -File) {
            $relativeFiles.Add([IO.Path]::GetRelativePath($SourceRoot, $file.FullName).Replace('\', '/'))
        }
        foreach ($file in Get-ChildItem -LiteralPath (Join-Path $SourceRoot 'skills') -Recurse -File) {
            $relativeFiles.Add([IO.Path]::GetRelativePath($SourceRoot, $file.FullName).Replace('\', '/'))
        }
        foreach ($file in Get-ChildItem -LiteralPath (Join-Path $SourceRoot 'docs/assets') -Filter '*.svg' -File) {
            $relativeFiles.Add([IO.Path]::GetRelativePath($SourceRoot, $file.FullName).Replace('\', '/'))
        }

        $portableManifest = Get-Content -LiteralPath (Join-Path $SourceRoot 'plugin.json') -Raw | ConvertFrom-Json -AsHashtable
        if ($portableManifest.ContainsKey('mcpServers') -or $portableManifest.ContainsKey('mcp')) {
            throw 'This release archive is intended for the skills-only plugin.'
        }
        if ($portableManifest.ContainsKey('extensions') -and $portableManifest.extensions.ContainsKey('com.openai')) {
            $interface = $portableManifest.extensions['com.openai'].interface
            foreach ($field in @('logo', 'composerIcon')) {
                if ($interface.ContainsKey($field)) {
                    $relative = ([string] $interface[$field]) -replace '^\./', ''
                    if ($relativeFiles -cnotcontains $relative) {
                        throw "The plugin's $field asset is not included in the archive: $relative"
                    }
                }
            }
        }
        $cursorManifest = Get-Content -LiteralPath (Join-Path $SourceRoot '.cursor-plugin/plugin.json') -Raw | ConvertFrom-Json -AsHashtable
        if ($cursorManifest.ContainsKey('logo') -and $relativeFiles -cnotcontains $cursorManifest.logo) {
            throw 'The Cursor listing logo is not included in the plugin archive.'
        }
        foreach ($relative in $relativeFiles) {
            if ($relative -match '(^|/)(\.git|target|node_modules|scripts)(/|$)' -or $relative -match '\\') {
                throw "Unexpected plugin archive path: $relative"
            }
            if (-not (Test-Path -LiteralPath (Join-Path $SourceRoot $relative) -PathType Leaf)) {
                throw "Required plugin file is missing: $relative"
            }
        }
        $archivePath = Join-Path $OutputDirectory "diffrail-plugin-$expectedTag.zip"
        New-PortableZip -ArchivePath $archivePath -Root $SourceRoot -RelativeFiles $relativeFiles.ToArray()
        $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
        try {
            if ($archive.Entries.Count -ne $relativeFiles.Count) { throw 'Plugin archive is incomplete.' }
            foreach ($entry in $archive.Entries) {
                if ($entry.FullName -match '\\|(^|/)\.\.(/|$)') { throw 'Plugin ZIP contains a non-portable path.' }
            }
        }
        finally { $archive.Dispose() }
        Write-Output "Created $archivePath"

        if ($IncludeCrate) {
            $cratePath = Join-Path $SourceRoot "target/package/diffrail-$version.crate"
            if (-not (Test-Path -LiteralPath $cratePath -PathType Leaf)) {
                throw 'Run cargo package --locked before packaging the crate artifact.'
            }
            $crateDestination = Join-Path $OutputDirectory "diffrail-$version.crate"
            if (Test-Path -LiteralPath $crateDestination) { throw "Refusing to overwrite $crateDestination" }
            Copy-Item -LiteralPath $cratePath -Destination $crateDestination
            Write-Output "Copied $crateDestination"
        }
    }

    if ($Mode -eq 'Binary') {
        if (-not $Target) { throw 'Binary packaging requires -Target.' }
        $isWindowsTarget = $Target -eq 'x86_64-pc-windows-msvc'
        $executableName = if ($isWindowsTarget) { 'diffrail.exe' } else { 'diffrail' }
        if (-not $BinaryPath) {
            $BinaryPath = Join-Path $SourceRoot "target/$Target/release/$executableName"
        }
        $BinaryPath = (Resolve-Path -LiteralPath $BinaryPath).Path
        $binaryVersion = & $BinaryPath --version
        if ($LASTEXITCODE -ne 0 -or ($binaryVersion -join "`n").Trim() -cne "diffrail $version") {
            throw "Binary version does not match diffrail $version."
        }
        $staging = New-StagingDirectory
        $directoryName = "diffrail-$expectedTag-$Target"
        $directory = [IO.Directory]::CreateDirectory((Join-Path $staging $directoryName)).FullName
        Copy-Item -LiteralPath $BinaryPath -Destination (Join-Path $directory $executableName)
        Copy-Item -LiteralPath (Join-Path $SourceRoot 'LICENSE') -Destination (Join-Path $directory 'LICENSE')
        $platformRequirements = switch ($Target) {
            'x86_64-unknown-linux-gnu' { 'Linux x64 with glibc 2.35 or newer; Alpine/musl is not supported by this archive.' }
            'x86_64-apple-darwin' { 'macOS Intel. This binary is unsigned and is not notarized.' }
            'aarch64-apple-darwin' { 'macOS Apple Silicon. This binary is unsigned and is not notarized.' }
            default { 'Windows x64.' }
        }
        $installInstructions = @"
DiffRail $version — $Target

$platformRequirements

1. Extract this archive and verify its checksum against SHA256SUMS.
2. Put $executableName in a directory on your PATH.
3. Run: diffrail --version
4. Run: diffrail init --help

Git must be installed. This binary checks file boundaries; it is not a sandbox.
The plugin is a separate download and does not install this executable.
Instructions and examples: https://github.com/beriktassuly/diffrail/tree/$expectedTag
License: MIT (see LICENSE).
"@
        Write-Utf8 -Path (Join-Path $directory 'INSTALL.txt') -Content ($installInstructions + "`n")
        if ($isWindowsTarget) {
            $archivePath = Join-Path $OutputDirectory "$directoryName.zip"
            $archiveFiles = @("$directoryName/$executableName", "$directoryName/LICENSE", "$directoryName/INSTALL.txt")
            New-PortableZip -ArchivePath $archivePath -Root $staging -RelativeFiles $archiveFiles
        }
        else {
            Invoke-Checked -Command 'chmod' -Arguments @('755', (Join-Path $directory $executableName))
            $archivePath = Join-Path $OutputDirectory "$directoryName.tar.gz"
            if (Test-Path -LiteralPath $archivePath) { throw "Refusing to overwrite $archivePath" }
            Invoke-Checked -Command 'tar' -Arguments @('-czf', $archivePath, '-C', $staging, $directoryName)
        }
        $extractRoot = [IO.Directory]::CreateDirectory((Join-Path $staging 'extracted')).FullName
        if ($isWindowsTarget) { [IO.Compression.ZipFile]::ExtractToDirectory($archivePath, $extractRoot) }
        else { Invoke-Checked -Command 'tar' -Arguments @('-xzf', $archivePath, '-C', $extractRoot) }
        $extractedBinary = Join-Path (Join-Path $extractRoot $directoryName) $executableName
        $extractedVersion = & $extractedBinary --version
        if ($LASTEXITCODE -ne 0 -or ($extractedVersion -join "`n").Trim() -cne "diffrail $version") {
            throw 'The packaged executable failed its extracted-archive smoke test.'
        }
        $installTest = Join-Path $SourceRoot '.github/scripts/test-install.ps1'
        if (-not (Test-Path -LiteralPath $installTest -PathType Leaf)) {
            throw 'The isolated CLI installation smoke-test helper is missing.'
        }
        & $installTest -ExecutablePath $extractedBinary -ExpectedVersion $version
        Write-Output "Created and smoke-tested $archivePath"
    }

    if ($Mode -eq 'Checksums') {
        $assets = @(Get-ChildItem -LiteralPath $OutputDirectory -File | Where-Object Name -ne 'SHA256SUMS' | Sort-Object Name)
        if ($assets.Count -eq 0) { throw 'No release artifacts were found.' }
        if ($RequireComplete) {
            $expectedAssets = @(
                "diffrail-$expectedTag-x86_64-pc-windows-msvc.zip",
                "diffrail-$expectedTag-x86_64-unknown-linux-gnu.tar.gz",
                "diffrail-$expectedTag-x86_64-apple-darwin.tar.gz",
                "diffrail-$expectedTag-aarch64-apple-darwin.tar.gz",
                "diffrail-plugin-$expectedTag.zip",
                "diffrail-$version.crate"
            )
            $difference = Compare-Object -ReferenceObject $expectedAssets -DifferenceObject @($assets.Name)
            if ($difference) { throw "Release asset set is incomplete or unexpected: $($difference | Out-String)" }
        }
        $lines = foreach ($asset in $assets) {
            if ($asset.Length -eq 0) { throw "Empty release artifact: $($asset.Name)" }
            $hash = (Get-FileHash -LiteralPath $asset.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            "$hash  $($asset.Name)"
        }
        $checksumsPath = Join-Path $OutputDirectory 'SHA256SUMS'
        Write-Utf8 -Path $checksumsPath -Content (($lines -join "`n") + "`n")
        foreach ($line in Get-Content -LiteralPath $checksumsPath) {
            $expectedHash, $name = $line -split '  ', 2
            $actualHash = (Get-FileHash -LiteralPath (Join-Path $OutputDirectory $name) -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($expectedHash -cne $actualHash) { throw "Checksum verification failed: $name" }
        }
        Write-Output "Created and verified $checksumsPath"
    }
    $global:LASTEXITCODE = 0
}
finally { Pop-Location }
