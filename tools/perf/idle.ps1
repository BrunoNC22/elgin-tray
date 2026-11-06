# Uso: ver docs/performance.md ("Como medir"). O daemon usa o config de $env:ELGIN_TRAY_CONFIG.
# Mede o daemon parado: amostra a cada 30 s por $Minutes minutos.
param([string]$Exe, [int]$Minutes = 10, [string]$Out)
$p = Start-Process -FilePath $Exe -PassThru
Start-Sleep -Seconds 20   # deixa conectar e estabilizar
$p.Refresh()
$cpu0 = $p.TotalProcessorTime.TotalMilliseconds
$t0 = Get-Date
$rows = @()
for ($i = 0; $i -lt ($Minutes * 2); $i++) {
  Start-Sleep -Seconds 30
  $p.Refresh()
  $rows += "{0:N0}s ws={1:N1}MB private={2:N1}MB cpu={3:N0}ms" -f ((Get-Date) - $t0).TotalSeconds, ($p.WorkingSet64/1MB), ($p.PrivateMemorySize64/1MB), ($p.TotalProcessorTime.TotalMilliseconds - $cpu0)
}
$elapsed = ((Get-Date) - $t0).TotalMilliseconds
$cpu = $p.TotalProcessorTime.TotalMilliseconds - $cpu0
$rows += "TOTAL: {0:N1} min, CPU {1:N0} ms = {2:N4}% de um núcleo; pico WS {3:N1} MB; handles {4}; threads {5}" -f ($elapsed/60000), $cpu, (100*$cpu/$elapsed), ($p.PeakWorkingSet64/1MB), $p.HandleCount, $p.Threads.Count
$rows | Out-File -Encoding utf8 $Out
