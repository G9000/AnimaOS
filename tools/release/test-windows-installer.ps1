param()
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'This installer regression requires Windows' }
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$runId = [Guid]::NewGuid().ToString('N')
$fixture = Join-Path $repository "target/installer-regression/$runId"
$product = "Anima Installer Regression $runId"
$publisher = "AnimaOS Packaging Regression $runId"
$runValue = "AnimaInstallerRegression$runId"
$binaryName = "anima-regression-$runId"
$uninstall = "HKCU:/Software/Microsoft/Windows/CurrentVersion/Uninstall/$product"
$productKey = "HKCU:/Software/$publisher/$product"
$login = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Run'
$previous = Join-Path $fixture 'previous install'
$destination = Join-Path $fixture 'new install'
New-Item -ItemType Directory -Path $fixture,$previous -Force | Out-Null

# Compile the production template with unique registration and startup names.
# This exercises real NSIS instructions without using the user's Anima keys.
$config = Get-Content -LiteralPath (Join-Path $repository 'target/native-packager/packager-win32-x64.json') -Raw | ConvertFrom-Json -AsHashtable
$config.productName = $product
$config.publisher = $publisher
$config.identifier = "dev.animaos.anima.regression.$runId"
$config.outDir = Join-Path $fixture 'packages'
$sourceBinary = Join-Path $config.binariesDir ($config.binaries[0].path + '.exe')
Copy-Item -LiteralPath $sourceBinary -Destination (Join-Path $fixture "$binaryName.exe")
$config.binariesDir = $fixture
$config.binaries = @(@{ path = $binaryName; main = $true })
$fixtureTemplate = Join-Path $fixture 'installer.nsi'
$template = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'nsis-installer.nsi') -Raw
[IO.File]::WriteAllText($fixtureTemplate, $template.Replace('"AnimaOS"', ('"' + $runValue + '"')))
$config.nsis.template = $fixtureTemplate
# Reproduce the state transition of an upgrade's old uninstaller: remove the
# previously matching startup value after .onInit captured its preference.
$config.nsis.preinstallSection = @"
Section SimulatedUpgradeUninstaller
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "$runValue"
SectionEnd
"@
$configPath = Join-Path $fixture 'packager.json'
$config | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $configPath -Encoding utf8
$tool = Join-Path $repository 'target/tools/bin/cargo-packager.exe'
if ($env:CARGO_PACKAGER_BIN) { $tool = $env:CARGO_PACKAGER_BIN }
$toolVersion = & $tool --version
if ($LASTEXITCODE -ne 0 -or $toolVersion.Trim() -ne 'cargo-packager 0.11.8') { throw 'The regression requires cargo-packager 0.11.8' }
& $tool --config $configPath
if ($LASTEXITCODE -ne 0) { throw 'Failed to compile the isolated installer fixture' }
$installer = Get-ChildItem -LiteralPath $config.outDir -Filter '*-setup.exe' | Select-Object -ExpandProperty FullName
if (-not $installer) { throw 'The installer fixture is missing' }
if ((Test-Path -LiteralPath $uninstall) -or (Test-Path -LiteralPath $productKey)) { throw 'Refusing to reuse an existing regression registration' }
$sentinel = Join-Path $previous "$binaryName.exe"
[IO.File]::WriteAllText($sentinel, 'Keep this newer installed binary unchanged')
$beforeHash = (Get-FileHash -LiteralPath $sentinel -Algorithm SHA256).Hash
$data = Join-Path $fixture 'personal-data.txt'
[IO.File]::WriteAllText($data, 'Keep this personal state unchanged')
$dataHash = (Get-FileHash -LiteralPath $data -Algorithm SHA256).Hash

function Invoke-FixtureInstaller([string] $Executable, [string[]] $Arguments) {
  $process = Start-Process -FilePath $Executable -ArgumentList $Arguments -WindowStyle Hidden -PassThru
  if (-not $process.WaitForExit(30000)) {
    # This is only the test's installer process, never an Anima runtime.
    Stop-Process -Id $process.Id
    throw 'The isolated installer did not finish within 30 seconds'
  }
  return $process.ExitCode
}

try {
  $registration = New-Item -Path $uninstall -Force
  $registration.SetValue('RegressionOwner', $runId)
  $registration.SetValue('DisplayVersion', '9.9.9')
  $registration.SetValue('InstallLocation', ('"' + $previous + '"'))
  $registration.Dispose()
  $exitCode = Invoke-FixtureInstaller $installer @('/S','/NS',"/D=$previous")
  if ($exitCode -ne 4) { throw "A silent downgrade must exit 4; received $exitCode" }
  if ((Get-FileHash -LiteralPath $sentinel -Algorithm SHA256).Hash -ne $beforeHash) { throw 'The rejected downgrade changed the installed binary' }
  $after = Get-ItemProperty -LiteralPath $uninstall
  if ($after.DisplayVersion -ne '9.9.9' -or $after.RegressionOwner -ne $runId) { throw 'The rejected downgrade changed its registration' }
  if (Test-Path -LiteralPath $productKey) { throw 'The rejected downgrade wrote a product registration' }

  # A matching enabled preference must survive the old uninstaller removing it,
  # then be retargeted to the new installation directory after successful setup.
  Set-ItemProperty -LiteralPath $uninstall -Name DisplayVersion -Value '0.0.0'
  if (-not (Test-Path -LiteralPath $login)) { New-Item -Path $login | Out-Null }
  New-ItemProperty -LiteralPath $login -Name $runValue -Value ('"' + $previous + '\' + $binaryName + '.exe" supervise --background') -PropertyType String | Out-Null
  $exitCode = Invoke-FixtureInstaller $installer @('/S','/NS',"/D=$destination")
  if ($exitCode -ne 0) { throw "The isolated upgrade failed with exit $exitCode" }
  $expected = '"' + $destination + '\' + $binaryName + '.exe" supervise --background'
  if ((Get-ItemPropertyValue -LiteralPath $login -Name $runValue) -ne $expected) { throw 'The upgrade lost or failed to retarget login startup' }
  if ((Get-FileHash -LiteralPath $data -Algorithm SHA256).Hash -ne $dataHash) { throw 'The upgrade changed personal data' }

  $exitCode = Invoke-FixtureInstaller (Join-Path $destination 'uninstall.exe') @('/S',"_?=$destination")
  if ($exitCode -ne 0) { throw "The isolated uninstaller failed with exit $exitCode" }
  if (Get-ItemProperty -LiteralPath $login -Name $runValue -ErrorAction SilentlyContinue) { throw 'Genuine uninstall kept its matching login registration' }
  if ((Get-FileHash -LiteralPath $data -Algorithm SHA256).Hash -ne $dataHash) { throw 'The uninstaller changed personal data' }
  Write-Output 'PASS: silent downgrade preserved files/registration; upgrade retained login startup across a changed install directory; genuine uninstall removed its matching entry and preserved personal state.'
} finally {
  # Every key/value below is uniquely named for this invocation. Never delete
  # the real Anima registration, an existing Run value, or another test's key.
  if (Test-Path -LiteralPath $uninstall) {
    $owner = Get-ItemPropertyValue -LiteralPath $uninstall -Name RegressionOwner -ErrorAction SilentlyContinue
    if ($owner -eq $runId) { Remove-Item -LiteralPath $uninstall -Recurse }
    else { throw "Unexpected owner at $uninstall; leaving it intact" }
  }
  if (Test-Path -LiteralPath $productKey) { Remove-Item -LiteralPath $productKey -Recurse }
  Remove-ItemProperty -LiteralPath $login -Name $runValue -ErrorAction SilentlyContinue
}
