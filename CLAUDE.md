# elgin-tray

App desktop nativo em Rust que controla uma lâmpada **Elgin Smart Color** (plataforma Tuya, Wi‑Fi)
pela bandeja do sistema (área de ícones ocultos) no **Windows 10/11** e no **Arch Linux com KDE Plasma 6**.

## Como trabalhar neste projeto (leia primeiro)

Todo o código é escrito pelo Claude, e as conversas podem ser limpas ou compactadas a qualquer momento.
**O estado do trabalho vive nas issues do GitHub**, não na conversa:

1. Veja o progresso: milestone **v1.0** (`gh api repos/BrunoNC22/elgin-tray/milestones --jq '.[] | {title, open_issues, closed_issues}'`)
   e a issue fixada **Roadmap v1.0** (#45, `gh issue view 45`), que lista as issues na ordem recomendada de execução.
2. Veja se já existe trabalho pela metade: `gh issue list --label "status:em-andamento"`. Se existir,
   continue por ela e leia os comentários (handoff do agente anterior).
3. Senão, pegue a próxima issue aberta do Roadmap cujas dependências (seção "Depende de") estejam fechadas.
   Confira a label de plataforma: `platform:windows` e `platform:linux` exigem estar na máquina certa.
   Adicione a label `status:em-andamento` ao começar.
4. Desenvolva com TDD (ver "Testes"). Commits pequenos direto na `main`, mensagens em português,
   referenciando a issue (`... (#12)`). Antes de cada push: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
   e `cargo test --workspace` passando.
5. Ao concluir: confira os critérios de aceite da issue, comente um resumo do que foi feito (decisões,
   arquivos principais, o que ficou para depois), remova `status:em-andamento` e feche a issue
   (ou use `Closes #N` no último commit). Marque o item correspondente no Roadmap.
6. Se a conversa precisar parar no meio de uma issue: comente na issue o estado atual e o próximo passo.
7. Decisões novas que valem para o projeto todo vão para este arquivo, na seção "Decisões".

Nunca versionar local key, Device ID real, user code do Smart Life, tokens nem nada de `config.toml`.
O repositório é **público**.

## Requisitos (v1.0)

- Inicia com o sistema e fica residente na bandeja, consumindo o mínimo possível de CPU e RAM.
- Ao iniciar, lê o estado da lâmpada: ligada/desligada, modo (branco ou cor), cor, brilho e temperatura.
- **Tudo é controlado pela bandeja** (não existe janela principal):
  - Clique esquerdo no ícone abre um **painel** colado na bandeja com: nome da lâmpada, switch liga/desliga,
    abas **Cor** (disco de cores: ângulo = matiz, distância do centro = saturação, centro branco) e
    **Branco** (temperatura quente ↔ frio), **slider de brilho** com gradiente do preto até a cor atual
    (no modo branco, até o branco na temperatura atual) e uma linha de **cores favoritas**.
  - O slider de brilho vai do mínimo da lâmpada (1%) até 100%. **Só o switch desliga** a lâmpada.
  - Clique direito abre o menu: ligar/desligar, favoritas, abrir painel, reconfigurar lâmpada,
    iniciar com o sistema, atualizações, sobre (versão) e sair.
  - Rolar o mouse sobre o ícone ajusta o brilho (só no KDE; a bandeja do Windows não envia rolagem).
  - O ícone mostra o estado: preenchido com a cor atual (ligada), contorno (desligada), cinza com
    indicador (offline / chave recusada / não configurada). Tooltip com nome, estado e brilho.
- **Cores favoritas:** 6 espaços; clique aplica, clique direito salva a cor atual no espaço.
- **Atalhos globais** (teclas configuráveis no `config.toml`): ligar/desligar, brilho +/− (10%),
  próxima cor favorita.
- **Configuração inicial dentro do app** por login com QR code do Smart Life (sem ferramentas Python):
  o usuário informa o user code, lê o QR com o app Smart Life, escolhe a lâmpada; o app salva Device ID,
  local key e a sessão da nuvem. O IP e a versão do protocolo são descobertos pela rede local.
- Se a lâmpada recusar a local key (ex.: foi pareada de novo), o app usa a **sessão salva** para buscar a
  chave nova sozinho; se a sessão também falhar, o ícone indica e o menu oferece "Reconfigurar lâmpada".
- **Atualização:** releases são criadas **só por tag de versão** (`vX.Y.Z`).
  - Windows: o app verifica ao iniciar e a cada 1 hora; se houver versão nova, **baixa, confere a assinatura
    digital, substitui o executável e reinicia sozinho**, mostrando uma notificação.
  - Arch: instalado pelo pacote AUR **`elgin-tray-bin`** (publicado pelo CI a cada tag). O app só **avisa**
    (notificação + item no menu) que há versão nova para atualizar pelo `yay`.
- v1.0 controla **uma** lâmpada, mas domínio e configuração já usam lista (preparado para várias).

## Decisões

| Tema | Decisão |
|---|---|
| Processos | **Dois processos, um executável.** `elgin-tray` (sem argumentos) é o daemon da bandeja: dono da conexão com a lâmpada, dos casos de uso e da configuração. `elgin-tray panel ...` é o painel (Slint), aberto sob demanda pelo daemon e encerrado ao fechar. Comunicação por IPC local. `elgin-tray ctl ...` é uma CLI de desenvolvimento. |
| UI | **Slint** com renderizador por software (sem GPU). |
| Bandeja | Windows: crate `tray-icon` (Win32). KDE: crate `ksni` (StatusNotifierItem via DBus, Rust puro, sem GTK). |
| Sessão Linux | Suportar KDE em **Wayland e X11**. Posicionamento do painel no Wayland definido pelo spike da issue correspondente. |
| Estrutura | **Workspace multi-crate** em `crates/`; a regra de dependência da arquitetura hexagonal é garantida pelo compilador. |
| Protocolo | Tuya LAN **3.5** implementado no projeto (não há crate Rust com 3.5). Referência: tinytuya. |
| Configuração | Arquivo `config.toml` na pasta de config do usuário (`%APPDATA%\elgin-tray\`, `~/.config/elgin-tray/`), permissão só do usuário. Guarda local key **e sessão da nuvem Tuya**. |
| Nuvem | Usada só no setup e na recuperação da local key. Controle do dia a dia é 100% LAN. |
| Releases | Só por tag `vX.Y.Z` (a versão do workspace precisa bater com a tag). Artefatos assinados com **zipsign**; a chave pública fica embutida no app. |
| Instalação | Windows: executável em `%LOCALAPPDATA%\elgin-tray\`, sem admin. Arch: AUR `elgin-tray-bin` (`/usr/bin/elgin-tray`). |
| Autostart | Windows: `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Linux: `~/.config/autostart/elgin-tray.desktop`. Ativado na primeira execução; alternável no menu. |
| Concorrência | `tokio` com runtime **`current_thread`** no daemon. Portas com `async fn` em traits (nativo no Rust 2024) sem exigir `Send` (`#![allow(async_fn_in_trait)]` na aplicação); casos de uso recebem adaptadores por **generics** (despacho estático). O domínio é síncrono. Testes assíncronos com `#[tokio::test]`. |
| Convenções da aplicação | Portas em `application::ports`, dublês em `application::testing` (compartilham estado entre clones via `Rc`, para o teste guardar um clone e inspecionar). A aplicação manda à lâmpada o **estado desejado** (`BulbGateway::apply(&BulbState)`), não o comando; o adaptador decide os DPs. Pushes da lâmpada entram por métodos do controlador (`on_reported_state`), chamados pelo loop do daemon. Tempo sempre via porta `Clock`. |
| Conexão e eventos | `BulbConnector::connect` devolve **dois lados**: o gateway (pedidos: `fetch_state`, `apply`), que fica com o controlador, e os `BulbEvents` (`StateReported`, `ConnectionLost`), que ficam com o supervisor. O loop do daemon espera `supervisor.next_event()` num `select!` e, em `ConnectionLost`, chama `supervisor.connection_lost`. No adaptador Tuya, um **ator** numa tarefa local do tokio é dono do socket (lê, faz heartbeat a cada 10 s, atende um pedido por vez, repassa pushes); por isso o daemon precisa rodar dentro de um **`tokio::task::LocalSet`**. |
| Comandos e energia | Comandos de cor, branco, temperatura e brilho **ligam** a lâmpada se ela estiver desligada (decisão do usuário). O brilho fica sempre entre 1% e 100%; só o liga/desliga apaga. Em cena/música, comandos de brilho só ligam a lâmpada. Regra em `BulbState::apply` (`crates/domain/src/command.rs`). |
| Favoritas | Guardam só **matiz e saturação**; aplicar mantém o brilho atual (100% se o modo não tem brilho). Só cores viram favorita (branco, cena e música não). "Próxima favorita" segue a cor atual em ciclo; se ela não é favorita, começa pela primeira. |
| Dublês de teste | **Escritos à mão** (stubs, spies, fakes), sem `mockall`. |
| Idioma | Código (tipos, funções, variáveis) em **inglês**. Comentários, docs, issues, commits e textos da interface em **português (pt-BR)**. |

## Arquitetura (hexagonal)

Detalhes, diagramas e o fluxo de um comando: [docs/architecture.md](docs/architecture.md).

```
crates/
  domain/          elgin-domain        Entidades e regras puras. Sem I/O, sem async, sem dependência de plataforma.
  application/     elgin-application   Casos de uso + portas (traits). Dublês das portas em `testing` (feature `testing`).
  tuya-lan/        elgin-tuya-lan      Adaptador: protocolo Tuya 3.5 (TCP 6668) e descoberta UDP.
  tuya-cloud/      elgin-tuya-cloud    Adaptador: login por QR, lista de dispositivos, local key (API device-sharing).
  config/          elgin-config        Adaptador: config.toml.
  ipc/             elgin-ipc           Protocolo de mensagens painel <-> daemon + transporte (named pipe / Unix socket).
  icons/           elgin-icons         Renderização do ícone da bandeja (RGBA) a partir do estado.
  tray-windows/    elgin-tray-windows  Adaptador: bandeja Windows.            (só compila no Windows)
  tray-kde/        elgin-tray-kde      Adaptador: bandeja KDE (ksni).         (só compila no Linux)
  platform-windows/ elgin-platform-windows  Autostart, atalhos, notificações, instalação de update. (Windows)
  platform-linux/  elgin-platform-linux    Autostart, atalhos (KGlobalAccel), notificações.        (Linux)
  updater/         elgin-updater       Adaptador: releases do GitHub + verificação de assinatura.
  panel/           elgin-panel         Painel Slint + view-model puro.
  app/             elgin-tray (bin)    Composition root: escolhe os adaptadores por `cfg(target_os)`.
```

- Regra de dependência: `domain` ← `application` ← adaptadores ← `app`. O domínio e a aplicação **não
  conhecem** Windows, Linux, Tuya, Slint, TOML nem rede. Adaptadores não dependem uns dos outros.
  Exceções: as bibliotecas de apoio `icons` (usada pelas bandejas) e `ipc` (usada pelo `panel` e pelo `app`).
- Crates de um sistema só têm conteúdo nele (`#![cfg(...)]` no `lib.rs`) e entram no `app` por
  `[target.'cfg(...)'.dependencies]`. Dependências compartilhadas ficam em `[workspace.dependencies]`
  (nas crates: `dep = { workspace = true }`); lints em `[workspace.lints]`.
- Para conferir o lado Linux a partir do Windows: `cargo clippy --workspace --all-targets --target x86_64-unknown-linux-gnu -- -D warnings`
  (alvo instalado com `rustup target add x86_64-unknown-linux-gnu`; só checa, não gera executável).
- Diferenças entre Windows e Linux ficam **só** nos adaptadores e na escolha feita pelo `app`
  (ex.: a política de atualização "instalar sozinho" vs "só avisar" é um valor injetado pelo `app`).
- Portas principais (driven): `BulbGateway`, `BulbDiscovery`, `ConfigRepository`, `CloudAccount`,
  `UpdateSource`, `UpdateInstaller`, `AutostartRegistrar`, `HotkeyRegistrar`, `Notifier`, `Clock`.
  Adaptadores driving: bandeja, IPC (comandos do painel), atalhos, CLI `ctl`.

## Testes

- **TDD**: teste falhando → código mínimo → refatorar. Princípios SOLID.
- Testes unitários **sempre com dublês de teste** para colaboradores (stubs, spies, fakes escritos à mão).
  Nada de rede, disco, relógio real, registro do Windows ou DBus em testes. Funções puras são testadas
  diretamente.
- O protocolo é testado com **vetores de teste** gerados com o tinytuya (`tools/`) usando uma chave falsa.
- A validação contra a lâmpada real é manual, pela CLI `elgin-tray ctl`, e registrada na issue.

## Protocolo da lâmpada

- Lâmpadas Elgin Smart são dispositivos Tuya genéricos (o app Elgin Smart é uma versão com a marca da Elgin
  em cima do SDK Tuya). Não é necessária engenharia reversa: usar o protocolo local Tuya.
- Protocolo local Tuya: TCP porta 6668, mensagens criptografadas com AES usando a **local key** (16 caracteres).
- **A lâmpada do usuário usa o protocolo 3.5** (confirmado com `tinytuya scan`).
  O 3.5 é diferente do 3.3: quadros com prefixo `0x00006699`, criptografia AES‑GCM e negociação de uma
  chave de sessão a cada conexão (handshake com nonces local/remoto, derivado da local key).
- Descoberta: a lâmpada 3.5 **não se anuncia sozinha**: é preciso mandar o pedido de descoberta (cmd `0x25`,
  cifrado com a chave UDP) em broadcast para a porta 7000, e ela responde na porta 7000 (cmd 19, formato 6699,
  com retcode). Lâmpadas mais antigas anunciam nas portas 6666 (texto) e 6667 (cifrado). Confirmado com uma
  captura real (`tools/tuya-vectors/capture_broadcast.py`).
- Dados necessários por lâmpada: `device_id`, `ip`, `local_key`, `version`.
- Comportamento confirmado na lâmpada real (#15): responde a consulta com cmd 16 e confirma comandos com
  cmd 13; manda push (cmd 8) a cada mudança, inclusive as feitas por outra conexão; aceita **pelo menos duas
  conexões locais ao mesmo tempo**; com a **chave errada, fecha a conexão** logo depois do pedido de
  negociação, sem responder (tratado como `KeyRejected`).
- **Firewall do Windows:** a descoberta precisa receber UDP da lâmpada (porta 7000). Na primeira execução o
  Windows pergunta se libera o executável; se for negado, a descoberta não acha nada (a conexão TCP, que é
  de saída, continua funcionando com o IP salvo).
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
  pareamento não basta.
- A local key muda se a lâmpada for pareada de novo.
- A lâmpada só usa Wi‑Fi 2,4 GHz e precisa estar na mesma sub‑rede do PC. O IP pode mudar (DHCP):
  usar a descoberta UDP para reencontrar a lâmpada pelo `device_id`.

## Referências

- tinytuya (Python, MIT), referência do protocolo LAN: https://github.com/jasonacox/tinytuya
- tuya-device-sharing-sdk (Python), referência do login por QR e da API de nuvem usada pelo Home Assistant:
  https://github.com/tuya/tuya-device-sharing-sdk
- tuya-local-key (Python, Apache-2.0), exemplo de uso do login por QR: https://github.com/vineetchoudhary/tuya-local-key
- Projeto parecido, só para Windows (PySide6), útil para comparar comportamento e DPs da Elgin:
  https://github.com/EnzoMVP/lamp-remote-control. Ele **não tem licença**, então não copiar código dele.

## Ambiente

- Desenvolvimento no Windows 10 (Git Bash/PowerShell) e no Arch Linux (KDE Plasma 6).
- Toolchain: Rust 1.99 (MSRV declarado em `rust-version` no `Cargo.toml`).
- O perfil de release usa LTO, `codegen-units = 1` e `strip` para gerar um binário pequeno.
- No Windows, o Python do Git Bash (MSYS) não instala pacotes com extensões nativas; use o Python nativo
  (`py -3.11`) para ferramentas como o tinytuya.
- Comandos: `cargo build`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt`.
