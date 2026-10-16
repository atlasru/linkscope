param([string]$Binary='target\debug\linkscope-desktop.exe',[switch]$KeepOpen)
$ErrorActionPreference='Stop'
$projectRoot=Split-Path -Parent $PSScriptRoot
$executable=(Resolve-Path -LiteralPath (Join-Path $projectRoot $Binary)).Path
$testDirectory=Join-Path $projectRoot ('target\native-tests\native-'+[guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testDirectory -Force | Out-Null
$resultFile=Join-Path $testDirectory 'result.json'
$application=Start-Process -FilePath $executable -ArgumentList @('--data-dir',('"{0}"' -f $testDirectory),'--smoke-test',('"{0}"' -f $resultFile)) -WindowStyle Hidden -PassThru
try {
  $deadline=(Get-Date).AddSeconds(60)
  while(!(Test-Path -LiteralPath $resultFile)) {
    if($application.HasExited){throw "Desktop exited with code $($application.ExitCode) before reporting a result"}
    if((Get-Date) -gt $deadline){throw 'Desktop smoke timed out'}
    Start-Sleep -Milliseconds 200
  }
  $result=Get-Content -Raw -LiteralPath $resultFile | ConvertFrom-Json
  $result | ConvertTo-Json -Depth 8
  Write-Output "Result file: $resultFile"
  if(!$result.ok){throw 'Native WebView/IPC smoke failed'}
  if($KeepOpen){$application.WaitForExit()}
} finally {
  if(!$KeepOpen -and !$application.HasExited){$application.CloseMainWindow() | Out-Null;if(!$application.WaitForExit(10000)){$application.Kill()}}
}
