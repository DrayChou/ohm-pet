$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Out = Join-Path $Root "dist/windows/OHM-Pet"
$Zip = Join-Path $Root "dist/windows/OHM-Pet-windows-x64-lite.zip"
$CollectionZip = Join-Path $Root "dist/windows/OHM-Pet-windows-x64-collection.zip"

Set-Location $Root
cargo build --release -p ohm-pet
$Exe = Join-Path $Root "target/release/ohm-pet.exe"
$Bytes = [System.IO.File]::ReadAllBytes($Exe)
$PeOffset = [System.BitConverter]::ToInt32($Bytes, 0x3C)
$Subsystem = [System.BitConverter]::ToUInt16($Bytes, $PeOffset + 24 + 68)
if ($Subsystem -ne 2) { throw "Expected Windows GUI subsystem (2), got $Subsystem" }
$EmbeddedIcon = [System.Drawing.Icon]::ExtractAssociatedIcon($Exe)
if ($null -eq $EmbeddedIcon) { throw "Windows executable has no embedded application icon" }
$EmbeddedIcon.Dispose()
if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item $Exe (Join-Path $Out "OHM Pet.exe")
Copy-Item -Recurse "assets/default-pets" (Join-Path $Out "pets")
Copy-Item "README.md" $Out
if (Test-Path $Zip) { Remove-Item -Force $Zip }
if (Test-Path $CollectionZip) { Remove-Item -Force $CollectionZip }
Compress-Archive -Path "$Out/*" -DestinationPath $Zip
Write-Output $Zip
$ExternalPets = Join-Path $Root "test-fixtures/external"
if (Test-Path $ExternalPets) {
  python scripts/make-collection-archive.py $Zip $ExternalPets $CollectionZip
  Write-Output $CollectionZip
}
