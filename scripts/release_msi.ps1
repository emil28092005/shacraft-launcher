param([Parameter(Mandatory=$true)][string]$PackagePath)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
# MSIDBOPEN_READONLY = 0. Reading COM properties never installs the package.
$installer = New-Object -ComObject WindowsInstaller.Installer
$summary = $installer.GetType().InvokeMember('SummaryInformation', 'GetProperty', $null, $installer, @($PackagePath, 0))
$template = $summary.GetType().InvokeMember('Property', 'GetProperty', $null, $summary, @(7))
$database = $installer.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $installer, @($PackagePath, 0))
$view = $database.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $database, @('SELECT `Value` FROM `Property` WHERE `Property` = ''ProductVersion'''))
$view.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $view, $null) | Out-Null
$record = $view.GetType().InvokeMember('Fetch', 'InvokeMethod', $null, $view, $null)
if ($null -eq $record) { throw 'Missing MSI ProductVersion' }
$version = $record.GetType().InvokeMember('StringData', 'GetProperty', $null, $record, @(1))
@{ template = [string]$template; version = [string]$version } | ConvertTo-Json -Compress
