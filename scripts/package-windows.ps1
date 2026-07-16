$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$Out = Join-Path $Root "dist/windows/OHM-Pet"
$Zip = Join-Path $Root "dist/windows/OHM-Pet-windows-x64.zip"

Set-Location $Root
cargo build --release -p ohm-pet
if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item "target/release/ohm-pet.exe" (Join-Path $Out "OHM Pet.exe")
Copy-Item -Recurse "pets" (Join-Path $Out "pets")
Copy-Item "README.md" $Out
if (Test-Path $Zip) { Remove-Item -Force $Zip }
Compress-Archive -Path "$Out/*" -DestinationPath $Zip
Write-Output $Zip
