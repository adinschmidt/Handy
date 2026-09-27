param([Parameter(Mandatory)][string]$Executable)

$ErrorActionPreference = 'Continue'
$directory = Split-Path -Parent $Executable
Write-Host "Inspecting Windows imports with $([System.Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture) PowerShell"
$files = @(Get-Item $Executable) + @(Get-ChildItem $directory -Filter '*.dll')
foreach ($file in $files) {
    Write-Host "Imports for $($file.FullName)"
    $module = [IntPtr]::Zero
    $moduleName = ''
    foreach ($line in (& llvm-readobj --coff-imports $file.FullName)) {
        if ($line -match '^  Name: (.+)$') {
            $moduleName = $Matches[1]
            $localPath = Join-Path $directory $moduleName
            $modulePath = if (Test-Path $localPath) { $localPath } else { $moduleName }
            try {
                $module = [System.Runtime.InteropServices.NativeLibrary]::Load($modulePath)
            } catch {
                $module = [IntPtr]::Zero
                Write-Host "Cannot load $modulePath : $_"
            }
        } elseif ($module -ne [IntPtr]::Zero -and $line -match '^  Symbol: (.+) \(\d+\)$') {
            $symbol = $Matches[1]
            $address = [IntPtr]::Zero
            if (-not [System.Runtime.InteropServices.NativeLibrary]::TryGetExport($module, $symbol, [ref]$address)) {
                Write-Host "MISSING EXPORT: $moduleName!$symbol imported by $($file.Name)"
            }
        } elseif ($line -eq '}' -and $module -ne [IntPtr]::Zero) {
            [System.Runtime.InteropServices.NativeLibrary]::Free($module)
            $module = [IntPtr]::Zero
        }
    }
}
