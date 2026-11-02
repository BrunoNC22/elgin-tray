# elgin-tray

Controle a sua lâmpada **Elgin Smart Color** (Tuya, Wi‑Fi) pela bandeja do sistema, no **Windows** e no
**Linux (KDE Plasma)**, sem o app de celular e sem passar pela nuvem.

> 🚧 **Em desenvolvimento.** Ainda não há código funcional.

## Recursos planejados

- Ícone na bandeja (área de ícones ocultos) que mostra o estado da lâmpada: ligada, desligada ou offline.
- Liga/desliga, brilho, temperatura de branco e cor direto pela bandeja.
- Lê o estado atual da lâmpada ao iniciar.
- Inicia junto com o sistema.
- Controle 100% local (protocolo Tuya na LAN), rápido e funcionando mesmo sem internet.
- Binário nativo em Rust, leve e com baixo consumo de memória.

## Requisitos

- Lâmpada Elgin Smart (ou outra lâmpada Tuya/Smart Life) conectada ao Wi‑Fi 2,4 GHz, na mesma rede do PC.
- O **Device ID**, o **IP** e a **local key** da lâmpada (veja abaixo).
- Windows 10/11 ou Linux com uma bandeja compatível com StatusNotifierItem (ex.: KDE Plasma).

## Obtendo a local key

A local key é a chave que o app usa para conversar com a lâmpada pela rede local. Só é preciso obtê‑la uma vez:

1. Pareie a lâmpada com o app **Smart Life** ou **Tuya Smart**. O app Elgin Smart pode não funcionar
   para este passo.
2. Obtenha a chave com o [tinytuya](https://github.com/jasonacox/tinytuya) (`python -m tinytuya wizard`,
   precisa de uma conta gratuita em [platform.tuya.com](https://platform.tuya.com)) ou com o
   [tuya-local-key](https://github.com/vineetchoudhary/tuya-local-key) (login por QR code, sem conta de
   desenvolvedor).

> ⚠️ Se a lâmpada for pareada de novo, a local key muda. Nunca compartilhe a sua local key.

## Compilando

```sh
cargo build --release
```

## Licença

A definir.
