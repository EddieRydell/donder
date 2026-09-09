$ErrorActionPreference = 'Stop'

# Build the controller image without opening a serial port or flashing a board.
$outputDirectory = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../target/firmware'))
$pendingImage = Join-Path $outputDirectory 'dawn-esp32.build.bin'
$finalImage = Join-Path $outputDirectory 'dawn-esp32.bin'

Push-Location $PSScriptRoot
try {
    . ./export-esp.ps1
    Get-Command cargo, espflash -ErrorAction Stop | Out-Null
    & cargo +esp build --release --bin loader --features i2s-output --locked
    if ($LASTEXITCODE -ne 0) { throw 'Controller firmware build failed.' }

    New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null
    & espflash save-image --skip-update-check --chip esp32 --flash-size 4mb --flash-mode dio --flash-freq 40mhz --xtal-freq 40mhz --partition-table partitions.csv --target-app-partition factory --merge --skip-padding target/xtensa-esp32-none-elf/release/loader $pendingImage
    if ($LASTEXITCODE -ne 0) { throw 'Controller image packaging failed.' }

    # Never include the persistent data partition in the installation image.
    # Derive its boundary from the same table passed to espflash.
    $dataPartition = Import-Csv -LiteralPath ./partitions.csv -Header Name, Type, SubType, Offset, Size, Flags |
        Where-Object { $_.Name.Trim() -eq 'dawn' }
    if (@($dataPartition).Count -ne 1) { throw 'Expected exactly one Dawn data partition.' }
    $dataOffset = [Convert]::ToInt64($dataPartition.Offset.Trim(), 16)
    $imageLength = (Get-Item -LiteralPath $pendingImage).Length
    if ($imageLength -le 0x10000 -or $imageLength -gt $dataOffset) {
        throw 'Packaged image is empty or overlaps the Dawn data partition.'
    }

    Move-Item -LiteralPath $pendingImage -Destination $finalImage -Force
    $bundledDirectory = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../../apps/desktop/assets/firmware'))
    New-Item -ItemType Directory -Path $bundledDirectory -Force | Out-Null
    Copy-Item -LiteralPath $finalImage -Destination (Join-Path $bundledDirectory 'dawn-esp32.bin') -Force
    $imageHash = (Get-FileHash -LiteralPath $finalImage -Algorithm SHA256).Hash
    Set-Content -LiteralPath (Join-Path $bundledDirectory 'dawn-esp32.sha256') -Value $imageHash -Encoding Ascii
    Write-Output "Controller image: $finalImage"
    Write-Output "Bytes: $imageLength"
    Write-Output "SHA256: $imageHash"
    Write-Output "Desktop firmware assets: $bundledDirectory"
}
finally {
    Pop-Location
}
