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
