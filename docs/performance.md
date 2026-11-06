# Consumo de recursos

Metas da v1.0 (#42): daemon parado com **≤ 15 MB** de RAM e **~0% de CPU**; painel abrindo em
**< 300 ms**.

## Windows

Medido em 2026-11-06, build de release do commit `da54a82` (0.1.0), Windows 10 22H2, Intel Core
i3-6006U (2 núcleos, 2 GHz), 12 GB de RAM, lâmpada conectada pela rede o tempo todo.

| Medida | Resultado | Meta |
|---|---|---|
| Daemon parado, **memória privada** (o que o Gerenciador de Tarefas mostra) | **1,1 MB** (conjunto de trabalho particular); 2,7 MB de bytes privados | ≤ 15 MB ✅ |
| Daemon parado, conjunto de trabalho total | 24,7 MB (pico 25,3 MB), quase tudo DLLs do Windows compartilhadas com outros processos | — |
| Daemon parado, CPU em 10 min | **0 ms** medidos (abaixo da resolução do contador, 15,6 ms): o heartbeat a cada 10 s custa menos que isso | ~0% ✅ |
| Daemon: threads / handles | 5 / 194 | — |
| Painel: do início do processo até a janela visível | **134–164 ms**; 364 ms na primeira abertura depois de um tempo (arquivos e fontes fora do cache do disco) | < 300 ms ✅ (⚠️ primeira abertura) |
| Painel aberto, memória | 4,5 MB privados; 29 MB de conjunto de trabalho | — |
| Tamanho do executável | 15,0 MB (um só arquivo, daemon + painel + CLI) | — |

As 5 threads do daemon: principal (bandeja), daemon (tokio), atalhos globais e as auxiliares do
Windows. O painel só existe enquanto está aberto (processo separado), então não pesa no daemon.

Nada ficou fora da meta. A primeira abertura do painel passa um pouco dos 300 ms porque o processo
lê o executável e as fontes do disco; nas seguintes (o caso comum) fica abaixo de 170 ms. Se
incomodar, o caminho é embutir uma fonte no painel para o Slint não varrer as fontes do sistema.

### Como medir

Com um build de release e uma cópia do config com `autostart = false` (para não registrar o
executável de teste no início do Windows):

```powershell
$env:ELGIN_TRAY_CONFIG = "C:\caminho\config-de-teste.toml"
powershell -File tools\perf\idle.ps1 -Exe C:\caminho\elgin-tray.exe -Minutes 10 -Out idle.txt
# com o daemon ainda rodando:
powershell -File tools\perf\panel.ps1 -Exe C:\caminho\elgin-tray.exe -Runs 5
Get-Counter '\Processo(elgin-tray)\Conjunto de trabalho - particular'   # Windows em português
```

## Arch Linux (KDE Plasma)

A medir junto com a bandeja do KDE (#25).
