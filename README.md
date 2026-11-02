# elgin-tray

[![CI](https://github.com/BrunoNC22/elgin-tray/actions/workflows/ci.yml/badge.svg)](https://github.com/BrunoNC22/elgin-tray/actions/workflows/ci.yml)

Controle a sua lâmpada **Elgin Smart Color** (Tuya, Wi‑Fi) pela bandeja do sistema, no **Windows** e no
**Linux (KDE Plasma)**, sem o app de celular e sem passar pela nuvem no dia a dia.

> 🚧 **Em desenvolvimento.** Ainda não há código funcional. Acompanhe o progresso no
> [Roadmap](https://github.com/BrunoNC22/elgin-tray/issues/45) e no
> [milestone v1.0](https://github.com/BrunoNC22/elgin-tray/milestone/1).

## Recursos planejados (v1.0)

- Ícone na bandeja (área de ícones ocultos) que mostra o estado da lâmpada: ligada (na cor atual), desligada
  ou offline.
- Clique no ícone abre um painel com switch liga/desliga, disco de cores (como no Smart Life), modo branco
  com temperatura quente ↔ frio, slider de brilho e cores favoritas.
- Menu no clique direito, rolagem do mouse sobre o ícone para ajustar o brilho (KDE) e atalhos globais de
  teclado.
- Lê o estado atual da lâmpada ao iniciar e acompanha mudanças feitas por outros apps.
- Inicia junto com o sistema.
- Configuração inicial dentro do app, lendo um QR code com o app Smart Life.
- Controle 100% local (protocolo Tuya 3.5 na LAN): rápido e funcionando mesmo sem internet.
- Atualização automática no Windows; no Arch Linux, pacote AUR `elgin-tray-bin`.
- Binário nativo em Rust, leve e com baixo consumo de memória.

## Requisitos

- Lâmpada Elgin Smart (ou outra lâmpada Tuya/Smart Life) conectada ao Wi‑Fi 2,4 GHz, na mesma rede do PC.
- A lâmpada precisa estar no app **Smart Life** (o app Elgin Smart não serve para a configuração inicial).
  Para mover a lâmpada para o Smart Life, **remova-a primeiro no app Elgin Smart** e depois pareie no
  Smart Life; sem isso, o Smart Life falha com "não foi possível conectar ao Wi‑Fi".
- Windows 10/11 ou Linux com KDE Plasma 6 (Wayland ou X11).

## Compilando

```sh
cargo build --release
```

## Usando pelo terminal (desenvolvimento)

Enquanto a bandeja não está pronta, a lâmpada pode ser controlada pelo terminal com `elgin-tray ctl`:

```sh
elgin-tray ctl status                 # mostra o estado
elgin-tray ctl on | off | toggle
elgin-tray ctl color 240 100 60       # matiz 0-359°, saturação 0-100%, brilho 1-100%
elgin-tray ctl white 50 0             # brilho 1-100%, temperatura 0 (quente) a 100 (fria)
elgin-tray ctl brightness 25          # brilho do modo atual
elgin-tray ctl discover               # procura a lâmpada na rede e salva o IP
elgin-tray ctl watch                  # mostra as mudanças até Ctrl+C
```

Ela lê a configuração de `%APPDATA%\elgin-tray\config.toml` (Windows) ou
`~/.config/elgin-tray/config.toml` (Linux). Para usar outro arquivo, defina a variável
`ELGIN_TRAY_CONFIG`. Enquanto a configuração pelo QR code não existe, preencha o arquivo à mão
(o ID e a local key podem ser obtidos com o [tuya-local-key](https://github.com/vineetchoudhary/tuya-local-key)):

```toml
[[bulbs]]
id = "o Device ID da lâmpada"
name = "Lâmpada do quarto"
local_key = "a local key (16 caracteres)"
```

O IP e a versão do protocolo são descobertos sozinhos e gravados no arquivo. A descoberta precisa
**receber** pacotes UDP da lâmpada: no Windows, permita o `elgin-tray.exe` no firewall quando o aviso
aparecer (redes privadas). Sem isso, preencha `ip` e `protocol_version = "3.5"` à mão.
