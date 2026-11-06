# Como publicar uma versão

As versões são publicadas **só por tag** `vX.Y.Z`. O workflow `.github/workflows/release.yml` faz o
resto: confere a versão, roda os testes, compila Windows e Linux, assina os pacotes e cria o GitHub
Release.

## Passo a passo

1. Atualize a versão em `Cargo.toml` (`[workspace.package] version = "X.Y.Z"`) e rode `cargo build`
   para atualizar o `Cargo.lock`.
2. Commit e push na `main`:
   ```sh
   git commit -am "Versão X.Y.Z"
   git push
   ```
3. Crie e envie a tag:
   ```sh
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```
4. Acompanhe em **Actions → Release**. No fim, o release `vX.Y.Z` aparece com:
   - `elgin-tray-vX.Y.Z-windows-x86_64.zip` (executável na raiz do zip);
   - `elgin-tray-vX.Y.Z-linux-x86_64.tar.gz` (executável, `.desktop` e ícone na raiz);
   - `SHA256SUMS.txt`;
   - notas geradas a partir dos commits.

Se a tag não bater com a versão do `Cargo.toml`, o workflow para logo no começo. Para testar o
workflow sem publicar uma versão de verdade, use uma tag com sufixo (`vX.Y.Z-test.1`): ela vira
**pré-release**, que o app não oferece como atualização. Depois apague:

```sh
gh release delete vX.Y.Z-test.1 --cleanup-tag --yes
git tag -d vX.Y.Z-test.1
```

## Assinatura (zipsign)

Os pacotes são assinados com o [zipsign](https://github.com/Kijewski/zipsign) (ed25519). A
assinatura fica dentro do próprio `.zip`/`.tar.gz` e usa o **nome do arquivo** como contexto: um
pacote renomeado não passa na verificação.

- **Chave pública:** `crates/updater/keys/zipsign.pub`, embutida no app (`elgin_updater::VERIFYING_KEY`).
  O app só instala atualizações assinadas pela chave privada correspondente.
- **Chave privada:** nunca entra no repositório. Fica:
  - no segredo `ZIPSIGN_PRIVATE_KEY` do GitHub (o arquivo de 64 bytes em base64);
  - e numa cópia de segurança do mantenedor (`%USERPROFILE%\.elgin-tray-release\zipsign.priv` no PC
    onde foi gerada). **Guarde essa cópia num lugar seguro** (gerenciador de senhas, pendrive): o
    GitHub não deixa ler o segredo de volta.

O workflow confere cada pacote com a chave pública do repositório logo depois de assinar; se o
segredo não for o par dela, a release falha antes de publicar.

### Cadastrar ou trocar o segredo

```sh
cargo install zipsign --version 0.2.2 --locked
zipsign gen-key zipsign.priv zipsign.pub      # só para um par novo
base64 -w0 zipsign.priv | gh secret set ZIPSIGN_PRIVATE_KEY
```

### Se a chave privada vazar ou se perder

Gere um par novo, troque `crates/updater/keys/zipsign.pub`, cadastre o segredo novo e publique uma
versão. Quem tem uma versão antiga instalada **não** consegue verificar pacotes da chave nova: no
Windows, essa pessoa precisa baixar e instalar a versão nova à mão uma vez. (No Arch, o pacote do AUR
não depende dessa assinatura.)

## Testar a atualização sem publicar nada

O app consulta `ELGIN_TRAY_RELEASES_URL` no lugar da API do GitHub, se a variável existir. Com um
servidor local dá para testar o ciclo inteiro (baixar, conferir a assinatura, trocar o executável e
reiniciar) no Windows:

1. Gere o executável "novo": mude a versão no `Cargo.toml` para uma maior (ex.: `0.1.1`), rode
   `cargo build -p elgin-tray`, copie `target/debug/elgin-tray.exe` para uma pasta `pkg/` e volte a
   versão.
2. Empacote e assine com a chave de verdade (o nome do arquivo faz parte da assinatura):
   ```sh
   cd pkg && python -c "import zipfile; zipfile.ZipFile('../server/elgin-tray-v0.1.1-windows-x86_64.zip','w',zipfile.ZIP_DEFLATED).write('elgin-tray.exe')"
   cd ../server && zipsign sign zip elgin-tray-v0.1.1-windows-x86_64.zip ~/.elgin-tray-release/zipsign.priv
   ```
3. Em `server/latest.json`:
   ```json
   {"tag_name": "v0.1.1", "assets": [{"name": "elgin-tray-v0.1.1-windows-x86_64.zip",
     "browser_download_url": "http://127.0.0.1:8765/elgin-tray-v0.1.1-windows-x86_64.zip"}]}
   ```
   e sirva a pasta: `python -m http.server 8765 --bind 127.0.0.1`.
4. Copie um build de **release** da versão atual (`cargo build --release`) para uma pasta de teste e
   rode com `ELGIN_TRAY_RELEASES_URL=http://127.0.0.1:8765/latest.json` (e `ELGIN_TRAY_CONFIG`
   apontando para uma cópia do config com `autostart = false`). Builds de debug nunca se atualizam.
5. Uns 30 s depois o log mostra `baixando ...`, `versão 0.1.1 instalada; reiniciando` e
   `elgin-tray 0.1.1 iniciando`. Para ver a recusa, troque um byte do `.zip` no meio: o log mostra
   `assinatura inválida` e o executável fica como estava.
