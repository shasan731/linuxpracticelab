<#
.SYNOPSIS
    Assembles the versioned runtime payload that ships inside the installer.

.DESCRIPTION
    Takes the verified QEMU download and the guest artefacts produced by the guest-image
    workflow, extracts only the files the application actually needs, and writes the
    checksums.json manifest the startup health check verifies against.

    The manifest is generated here rather than written by hand for one reason: a hand-maintained
    checksum list drifts, and a drifted list means the application refuses to start on a
    perfectly good installation.

.PARAMETER RuntimeVersion
    Version directory to build under runtime/bin. Defaults to the version in Cargo.toml.
#>

[CmdletBinding()]
param(
    [string]$RuntimeVersion,
    [string]$QemuInstaller = 'runtime/vendor/qemu-installer.exe',
    [string]$GuestDir = 'runtime/vendor/guest'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    $manifestPath = Join-Path $repoRoot 'runtime/qemu-manifest.json'
    $manifest = Get-Content $manifestPath -Raw | ConvertFrom-Json

    if (-not $RuntimeVersion) {
        $cargo = Get-Content (Join-Path $repoRoot 'Cargo.toml') -Raw
        if ($cargo -notmatch '(?m)^version = "([^"]+)"') {
            throw 'could not read the workspace version from Cargo.toml'
        }
        $RuntimeVersion = $Matches[1]
    }
    Write-Verbose "runtime version $RuntimeVersion"

    $outDir = Join-Path $repoRoot "runtime/bin/$RuntimeVersion"
    if (Test-Path $outDir) { Remove-Item -Recurse -Force $outDir }
    New-Item -ItemType Directory -Force $outDir | Out-Null
    New-Item -ItemType Directory -Force (Join-Path $outDir 'licences') | Out-Null

    # ---------------------------------------------------------------------
    # QEMU
    # ---------------------------------------------------------------------
    if (-not (Test-Path $QemuInstaller)) {
        throw "the QEMU installer is missing at $QemuInstaller. The Windows build workflow downloads and verifies it first."
    }

    # Re-verify here as well as in CI: this script is also run by hand during development, and
    # an unverified hypervisor binary is not something to be relaxed about.
    $actualHash = (Get-FileHash -Algorithm SHA256 $QemuInstaller).Hash.ToLower()
    $expectedHash = $manifest.source.sha256.ToLower()
    if ($expectedHash -eq ('0' * 64)) {
        throw 'runtime/qemu-manifest.json still holds the placeholder checksum; refusing to package an unverified hypervisor'
    } elseif ($actualHash -ne $expectedHash) {
        throw "the QEMU installer does not match the pinned checksum. Expected $expectedHash, found $actualHash."
    }
    if ($manifest.source.sha512PublishedByVendor) {
        $actualSha512 = (Get-FileHash -Algorithm SHA512 $QemuInstaller).Hash.ToLower()
        if ($actualSha512 -ne $manifest.source.sha512PublishedByVendor.ToLower()) {
            throw 'the QEMU installer does not match the SHA-512 checksum published by its vendor'
        }
    }

    $extractDir = Join-Path $repoRoot 'runtime/vendor/qemu-extracted'
    if (Test-Path $extractDir) { Remove-Item -Recurse -Force $extractDir }
    New-Item -ItemType Directory -Force $extractDir | Out-Null

    # The QEMU Windows installer is an NSIS package, which 7-Zip can unpack without running it.
    $sevenZip = Get-Command 7z -ErrorAction SilentlyContinue
    if (-not $sevenZip) {
        $candidate = 'C:\Program Files\7-Zip\7z.exe'
        if (Test-Path $candidate) {
            $sevenZip = @{ Source = $candidate }
        } else {
            throw '7-Zip is required to unpack the QEMU installer without executing it. Install it with: choco install 7zip'
        }
    }

    Write-Verbose 'unpacking the QEMU installer'
    & $sevenZip.Source x $QemuInstaller "-o$extractDir" -y | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "unpacking the QEMU installer failed with exit code $LASTEXITCODE" }

    function Copy-Required {
        param([string[]]$Names, [string]$Destination, [switch]$Optional)
        foreach ($name in $Names) {
            $found = Get-ChildItem -Path $extractDir -Filter $name -Recurse -File |
                Select-Object -First 1
            if (-not $found) {
                if ($Optional) {
                    Write-Warning "$name was not found in the QEMU package; continuing"
                    continue
                }
                throw "$name was not found in the QEMU package"
            }
            Copy-Item $found.FullName -Destination $Destination -Force
            Write-Verbose "  $name"
        }
    }

    Copy-Required -Names $manifest.extract.binaries -Destination $outDir
    $firmwareDir = Join-Path $outDir 'share'
    New-Item -ItemType Directory -Path $firmwareDir -Force | Out-Null
    Copy-Required -Names $manifest.extract.firmwareFiles -Destination $firmwareDir

    # QEMU's DLL dependency set changes between builds. Copy every shared library beside the
    # selected x86_64 executable, rather than maintaining a brittle hand-written subset that
    # can produce an installer which builds successfully but fails at process startup.
    $qemuSystem = Get-ChildItem -Path $extractDir -Filter 'qemu-system-x86_64.exe' -Recurse -File |
        Select-Object -First 1
    if (-not $qemuSystem) {
        throw 'qemu-system-x86_64.exe disappeared after extraction'
    }
    $dlls = Get-ChildItem -Path $qemuSystem.Directory.FullName -Filter '*.dll' -File
    if (-not $dlls) {
        throw "no QEMU runtime DLLs were found beside $($qemuSystem.FullName)"
    }
    foreach ($dll in $dlls) {
        Copy-Item $dll.FullName -Destination $outDir -Force
        Write-Verbose "  $($dll.Name)"
    }
    Copy-Required -Names $manifest.extract.licenceFiles -Destination (Join-Path $outDir 'licences') -Optional

    # This catches a missing transitive DLL immediately, while the exact extracted package is
    # still available for diagnosis.
    & (Join-Path $outDir 'qemu-system-x86_64.exe') --version | Write-Verbose
    if ($LASTEXITCODE -ne 0) {
        throw "the packaged qemu-system-x86_64.exe failed its startup smoke test"
    }
    & (Join-Path $outDir 'qemu-img.exe') --version | Write-Verbose
    if ($LASTEXITCODE -ne 0) {
        throw "the packaged qemu-img.exe failed its startup smoke test"
    }

    # ---------------------------------------------------------------------
    # Guest image
    # ---------------------------------------------------------------------
    foreach ($name in @('vmlinuz', 'initrd.img', 'debian-base.raw.zst', 'image-version')) {
        $source = Join-Path $GuestDir $name
        if (-not (Test-Path $source)) {
            if ($name -eq 'initrd.img') {
                Write-Warning 'no initrd.img: the kernel must have virtio-blk and ext4 built in'
                continue
            }
            throw "the guest artefact $name is missing from $GuestDir"
        }
        Copy-Item $source -Destination $outDir -Force
        Write-Verbose "  $name"
    }

    Copy-Item (Join-Path $repoRoot 'docs/licensing/*') -Destination (Join-Path $outDir 'licences') -Recurse -Force -ErrorAction SilentlyContinue
    $debianCopyright = Join-Path $GuestDir 'debian-copyright.txt'
    if (-not (Test-Path $debianCopyright)) {
        throw "the guest artefact debian-copyright.txt is missing from $GuestDir"
    }
    Copy-Item $debianCopyright -Destination (Join-Path $outDir 'licences') -Force

    # ---------------------------------------------------------------------
    # Manifest
    # ---------------------------------------------------------------------
    $imageVersion = 'unknown'
    $imageVersionFile = Join-Path $outDir 'image-version'
    if (Test-Path $imageVersionFile) {
        $imageVersion = (Get-Content $imageVersionFile -Raw).Trim()
    }

    $files = @()
    foreach ($file in Get-ChildItem -Path $outDir -Recurse -File) {
        $relative = $file.FullName.Substring($outDir.Length + 1).Replace('\', '/')
        if ($relative -eq 'checksums.json') { continue }
        $files += [pscustomobject][ordered]@{
            path       = $relative
            sha256     = (Get-FileHash -Algorithm SHA256 $file.FullName).Hash.ToLower()
            size_bytes = $file.Length
            # initrd is genuinely optional. The compressed image is verified during install,
            # then removed after it has materialised the required raw image.
            optional   = ($relative -eq 'initrd.img' -or $relative -eq 'debian-base.raw.zst')
        }
    }

    # The raw image is materialised from the shipped .zst on first launch. It is still a
    # required runtime file, so record the build-time size and hash and verify it after
    # decompression just like files copied directly from the installer.
    $imageManifestPath = Join-Path $GuestDir 'image-manifest.json'
    if (-not (Test-Path $imageManifestPath)) {
        throw "the guest artefact image-manifest.json is missing from $GuestDir"
    }
    $imageManifest = Get-Content $imageManifestPath -Raw | ConvertFrom-Json
    $files += [pscustomobject][ordered]@{
        path       = [string]$imageManifest.rawImage.path
        sha256     = [string]$imageManifest.rawImage.sha256
        size_bytes = [uint64]$imageManifest.rawImage.sizeBytes
        optional   = $false
    }

    $checksums = [ordered]@{
        runtime_version = $RuntimeVersion
        qemu_version    = $manifest.version
        image_version   = $imageVersion
        files           = $files
    }

    $checksumPath = Join-Path $outDir 'checksums.json'
    $checksumJson = $checksums | ConvertTo-Json -Depth 6
    $utf8WithoutBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($checksumPath, $checksumJson, $utf8WithoutBom)

    # The manifest also describes the 3 GiB raw image produced on first launch. It is not
    # shipped in the installer, so report the actual packaged bytes for the installer budget.
    $packagedBytes = (Get-ChildItem -Path $outDir -Recurse -File |
            Measure-Object -Property Length -Sum).Sum
    $totalMb = [math]::Round(($packagedBytes / 1MB), 1)
    Write-Host ''
    Write-Host "Runtime payload assembled at $outDir"
    Write-Host "  QEMU          $($manifest.version)"
    Write-Host "  guest image   $imageVersion"
    Write-Host "  files         $($files.Count)"
    Write-Host "  total size    $totalMb MB"
    if ($totalMb -gt 500) {
        Write-Warning "the payload exceeds the 500 MB installer budget from the specification"
    }
}
finally {
    Pop-Location
}
