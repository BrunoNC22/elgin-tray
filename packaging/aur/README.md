# Pacote AUR `elgin-tray-bin`

O `PKGBUILD` daqui é a fonte do pacote. A cada tag `vX.Y.Z` (release de verdade, sem sufixo), o job
`aur` do `.github/workflows/release.yml` troca o `pkgver`, recalcula o `sha256sums` a partir do
`.tar.gz` recém-publicado, gera o `.SRCINFO`, testa o `makepkg` e faz o push para
`ssh://aur@aur.archlinux.org/elgin-tray-bin.git`. O primeiro push cria o pacote no AUR.

Sem os segredos abaixo, o job só avisa (`::warning::`) e o release continua normal.

## Configurar a publicação (uma vez)

1. Crie uma conta em <https://aur.archlinux.org/register>.
2. Gere uma chave SSH só para o CI (sem senha, porque o CI usa sem ninguém digitar):
   ```sh
   ssh-keygen -t ed25519 -f ~/.ssh/aur-elgin-tray -N "" -C "elgin-tray CI"
   ```
3. No AUR, em **My Account → SSH Public Key**, cole o conteúdo de `~/.ssh/aur-elgin-tray.pub`.
4. Cadastre os segredos no GitHub (o e-mail e o nome vão no autor dos commits no AUR):
   ```sh
   gh secret set AUR_SSH_PRIVATE_KEY < ~/.ssh/aur-elgin-tray
   gh secret set AUR_USERNAME --body "seu-usuario-do-aur"
   gh secret set AUR_EMAIL --body "seu-email"
   ```
5. A próxima tag `vX.Y.Z` publica o pacote. Depois: `yay -S elgin-tray-bin`.

## Testar o PKGBUILD sem publicar

Monte o `.tar.gz` como o CI monta (arquivos na raiz) e aponte o `source` para ele:

```sh
cargo build --release -p elgin-tray
mkdir -p /tmp/aur-teste/stage && cd /tmp/aur-teste
cp ~/caminho/elgin-tray/{target/release/elgin-tray,assets/elgin-tray.desktop,assets/elgin-tray.png,README.md} stage/
(cd stage && tar -czf ../elgin-tray-v0.1.0-linux-x86_64.tar.gz *)
sed 's|^source=.*|source=("elgin-tray-v$pkgver-linux-x86_64.tar.gz")|' \
    ~/caminho/elgin-tray/packaging/aur/PKGBUILD > PKGBUILD
updpkgsums && makepkg -f
tar -tf elgin-tray-bin-*.pkg.tar.zst   # conferir o conteúdo; instalar com `sudo pacman -U`
```

O `.tar.gz` do release é assinado pelo `zipsign` (a assinatura fica dentro do próprio gzip); o
`makepkg` extrai normalmente (conferido com uma chave descartável).

## Dependências

`depends` vem do que o executável usa (spike #26): `fontconfig`/`freetype2` ligados diretamente;
`wayland`, `libxkbcommon` e, na sessão X11, `libx11`, `libxcursor`, `libxi` e `libxkbcommon-x11`
carregados em tempo de execução. O KWin e o Plasma (posição do painel, bandeja, atalhos) são usados
pelo DBus quando existem; sem eles o app funciona com menos recursos, por isso não entram em
`depends`.
