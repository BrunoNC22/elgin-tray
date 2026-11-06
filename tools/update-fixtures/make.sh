#!/usr/bin/env bash
# Gera os pacotes de teste do atualizador (crates/updater/tests/fixtures) com um par de chaves SÓ
# de teste. A chave privada de teste é jogada fora no fim; só a pública fica no repositório.
#
# Uso (na raiz do repositório): bash tools/update-fixtures/make.sh
# Precisa do zipsign 0.2.2 (`cargo install zipsign --version 0.2.2 --locked`) e do Python 3
# (`PYTHON=...` para outro interpretador).
set -euo pipefail
python=${PYTHON:-python3}  # ex.: PYTHON=python no Windows
out=crates/updater/tests/fixtures
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

zipsign gen-key "$work/test.priv" "$out/test-key.pub" --force
name=elgin-tray-v9.9.9-windows-x86_64.zip
"$python" - "$work/$name" <<'PY'
import sys, zipfile
with zipfile.ZipFile(sys.argv[1], "w", zipfile.ZIP_STORED) as z:
    z.writestr("elgin-tray.exe", "executável de teste v9.9.9\n")
    z.writestr("README.md", "leia-me\n")
PY
zipsign sign zip "$work/$name" "$work/test.priv"
cp "$work/$name" "$out/$name"
# Adulterado: troca um byte do executável (guardado sem compressão), mantendo o resto igual.
"$python" - "$out/$name" "$out/tampered-$name" <<'PY'
import sys
data = bytearray(open(sys.argv[1], "rb").read())
index = data.index(b"v9.9.9\n")
data[index + 1] = ord("8")
open(sys.argv[2], "wb").write(data)
PY
echo "fixtures gravados em $out"
