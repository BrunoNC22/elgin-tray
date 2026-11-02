# elgin-tray

App desktop nativo em Rust que controla uma lâmpada **Elgin Smart Color** (plataforma Tuya, Wi‑Fi)
pela bandeja do sistema (área de ícones ocultos) no **Windows 10/11** e no **Arch Linux com KDE Plasma**.

## Objetivo

- Iniciar junto com o sistema e ficar residente na bandeja, consumindo o mínimo possível de CPU e RAM.
- Ao iniciar, ler o estado atual da lâmpada: ligada/desligada, modo (branco ou cor), cor, brilho e
  temperatura de branco.
- Permitir alterar esse estado pelo próprio ícone/menu da bandeja.
- Funcionar **100% pela rede local**. A nuvem Tuya só é usada uma vez, fora do app, para obter a local key.

## Protocolo da lâmpada

- Lâmpadas Elgin Smart são dispositivos Tuya genéricos (o app Elgin Smart é uma versão com a marca da Elgin
  em cima do SDK Tuya). Não é necessária engenharia reversa: usar o protocolo local Tuya.
- Protocolo local Tuya: TCP porta 6668, mensagens criptografadas com AES usando a **local key** (16 caracteres).
- **A lâmpada do usuário usa o protocolo 3.5** (confirmado com `tinytuya scan`, Product ID `keytfesxxsqewnqy`).
  O 3.5 é diferente do 3.3: quadros com prefixo `0x00006699`, criptografia AES‑GCM e negociação de uma
  chave de sessão a cada conexão (handshake com nonces local/remoto, derivado da local key). As crates Rust
  existentes costumam suportar só 3.1/3.3, então o protocolo 3.5 provavelmente será implementado aqui,
  usando o tinytuya como referência e para validar o comportamento.
- Descoberta: broadcasts UDP nas portas 6666 (sem criptografia), 6667 (criptografados) e 7000 (3.5).
- Dados necessários por lâmpada: `device_id`, `ip`, `local_key`, `version`.
- DPs (data points) **confirmados na lâmpada real** (modelo "10W Smart color", categoria `dj`, PID na nuvem
  `ceut0xbogrwi6jnq`; o broadcast UDP anuncia outro product key, `keytfesxxsqewnqy`):
  - `20` liga/desliga (bool)
  - `21` modo: `white` | `colour` | `scene` | `music`
  - `22` brilho do branco (10–1000)
  - `23` temperatura do branco (0 = quente, 1000 = frio)
  - `24` cor em HSV como string hex de 12 caracteres: `HHHHSSSSVVVV` (H 0–360, S 0–1000, V 0–1000).
    Ex.: `000003e803e8` = vermelho, saturação e brilho máximos
  - `25` cena (hex), `26` temporizador em segundos (0–86400), `27` música, `28` controle em tempo real
  - `34` não perturbe (bool), `41` controle remoto habilitado (bool)
  - Exemplo de resposta de status pela LAN: `{'20': True, '21': 'white', '22': 1000, '23': 1000,
    '24': '000003e803e8', '25': '...', '26': 0, '34': False, '41': True}`
- **Pareamento no Smart Life:** a lâmpada Elgin funciona no app Smart Life (aparece como "10W Smart color"),
  mas antes é preciso **remover o dispositivo no app Elgin Smart** (editar → "Remover dispositivo").
  Enquanto ela continua vinculada à conta Elgin na nuvem, o Smart Life falha com a mensagem genérica
  "não foi possível conectar ao Wi‑Fi", mesmo com a rede e a senha corretas. Colocar a lâmpada em modo de
  pareamento não basta. Com a lâmpada no Smart Life, a local key pode ser obtida pelo wizard do tinytuya
  ou pelo tuya-local-key.
- A local key muda se a lâmpada for pareada de novo. O app deve mostrar um erro claro quando a chave
  for recusada, em vez de travar.
- A lâmpada só usa Wi‑Fi 2,4 GHz e precisa estar na mesma sub‑rede do PC. O IP pode mudar (DHCP):
  usar a descoberta UDP para reencontrar a lâmpada pelo `device_id`.

## Referências

- tinytuya (Python), referência do protocolo: https://github.com/jasonacox/tinytuya
- Projeto parecido, só para Windows (PySide6), útil para comparar comportamento e DPs da Elgin:
  https://github.com/EnzoMVP/lamp-remote-control. Ele **não tem licença**, então não copiar código dele.
- Obter a local key sem conta de desenvolvedor: https://github.com/vineetchoudhary/tuya-local-key

## Diretrizes técnicas

- Linguagem: Rust (edição 2024). Um único código para os dois sistemas; código específico de cada
  sistema fica isolado com `#[cfg(target_os = ...)]`.
- Bandeja: no KDE usar StatusNotifierItem/DBus (ex.: crate `ksni`), não o XEmbed antigo. No Windows,
  usar o ícone de notificação do Win32 (ex.: crates `tray-icon` ou `windows`). Avaliar com cuidado antes
  de adicionar dependências pesadas (GTK, webview, runtime de UI completo).
- Baixo consumo: nada de busy loop. Consultar o estado com intervalo longo e/ou manter uma conexão
  persistente com heartbeat. Ficar ocioso quando não houver interação. Medir a RAM das builds de release.
- Início automático: Windows em `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; Linux com um
  arquivo `.desktop` em `~/.config/autostart/` (ou um serviço de usuário do systemd).
- Configuração: arquivo TOML no diretório de config do usuário (`%APPDATA%\elgin-tray\` /
  `~/.config/elgin-tray/`). **Nunca** versionar local keys, device IDs reais nem credenciais.
- Lógica pura (codificação de DPs, criptografia, parsing de mensagens) separada da UI e da rede, com testes
  unitários que não dependem de uma lâmpada real.
- Rede instável é o caso normal: timeouts, reconexão com backoff e estado "offline" visível no ícone.

## Ambiente

- Desenvolvimento no Windows 10 (Git Bash/PowerShell) e no Arch Linux (KDE Plasma).
- Toolchain: Rust 1.99 (MSRV declarado em `rust-version` no `Cargo.toml`).
- O perfil de release usa LTO, `codegen-units = 1` e `strip` para gerar um binário pequeno.
- Comandos: `cargo build`, `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt`.
