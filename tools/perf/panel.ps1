# Uso: ver docs/performance.md ("Como medir"). O daemon usa o config de $env:ELGIN_TRAY_CONFIG.
# Abre o painel $Runs vezes: tempo do início do processo até a janela aparecer, e a RAM dele.
param([string]$Exe, [int]$Runs = 5)
Add-Type -Namespace P -Name U -MemberDefinition '[DllImport("user32.dll")] public static extern System.IntPtr FindWindowW(string c, [MarshalAs(UnmanagedType.LPWStr)] string t); [DllImport("user32.dll")] public static extern bool IsWindowVisible(System.IntPtr h);'
for ($i = 0; $i -lt $Runs; $i++) {
  $sw = [Diagnostics.Stopwatch]::StartNew()
  $p = Start-Process -FilePath $Exe -ArgumentList "panel" -PassThru
  $h = [System.IntPtr]::Zero
  while ($sw.ElapsedMilliseconds -lt 5000) {
    $h = [P.U]::FindWindowW([NullString]::Value, "elgin-tray")
    if ($h -ne [System.IntPtr]::Zero -and [P.U]::IsWindowVisible($h)) { break }
    Start-Sleep -Milliseconds 5
  }
  $visible = $sw.ElapsedMilliseconds
  Start-Sleep -Milliseconds 1500
  $p.Refresh()
  "abertura {0} ms; ws={1:N1}MB private={2:N1}MB" -f $visible, ($p.WorkingSet64/1MB), ($p.PrivateMemorySize64/1MB)
  Stop-Process -Id $p.Id -Force
  Start-Sleep -Milliseconds 800
}
