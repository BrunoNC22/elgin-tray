# Vetores de teste do protocolo Tuya 3.5

Gera `crates/tuya-lan/tests/fixtures/vectors.json` usando o [tinytuya](https://github.com/jasonacox/tinytuya)
(versão fixada em `requirements.txt`) como implementação de referência. Os testes do `elgin-tuya-lan`
comparam a implementação em Rust byte a byte com esses vetores.

Tudo usa **chave falsa** (`0123456789abcdef`), Device ID falso e nonces/IVs fixos: a saída é
determinística e não há nenhum dado real. Nenhuma rede é acessada.

## Como gerar

Na raiz do repositório, no Windows (use o Python nativo; o Python do Git Bash/MSYS não instala o
`cryptography`):

```sh
py -3.11 -m venv .venv-vectors
.venv-vectors/Scripts/python -m pip install -r tools/tuya-vectors/requirements.txt
.venv-vectors/Scripts/python -I tools/tuya-vectors/generate.py crates/tuya-lan/tests/fixtures/vectors.json
```

No Linux, troque `Scripts/python` por `bin/python`.

## O que é gerado

| Seção | Conteúdo | Funções do tinytuya usadas |
|---|---|---|
| `session` | local/remote nonce, os dois HMACs, a chave de sessão e os quadros 3, 4 e 5 da negociação | `XenonDevice._negotiate_session_key_generate_step_3`, `_negotiate_session_key_generate_finalize`, `pack_message` |
| `frames` | quadros cifrados com a chave de sessão: consulta (16) e resposta, heartbeat (9) e resposta, comando (13) e confirmação, push de estado (8) | `pack_message` / `unpack_message` (formato 6699) |
| `udp` | broadcasts 3.5 (formato 6699, com e sem retcode), broadcast 3.3 (formato 55AA, AES-ECB) e o pedido de descoberta enviado à porta 7000 | `pack_message`, `udp_helper.encrypt`, `udp_helper.decrypt_udp` |

Cada vetor guarda o `plaintext` (o que vai dentro da criptografia, incluindo retcode e cabeçalho de
versão quando existem) e o `frame`/`datagram` completo, tudo em hexadecimal.

O gerador confere cada vetor com o próprio tinytuya (desempacota de volta e compara) antes de gravar.

## Formato 6699 (protocolo 3.5), como implementado no tinytuya 1.20.0

```
prefixo 00006699 | reservado u16 = 0 | seqno u32 | cmd u32 | length u32 |
IV (12) | ciphertext | tag GCM (16) | sufixo 00009966
```

- `length` = 12 (IV) + tamanho do ciphertext + 16 (tag). Tudo big-endian.
- AES-128-GCM; dados adicionais autenticados (AAD) = os 14 bytes do header depois do prefixo.
- Mensagens da lâmpada trazem um **retcode** (u32) no início do texto puro.
- Comandos fora de `NO_PROTOCOL_HEADER_CMDS` (ex.: `CONTROL_NEW` = 13) levam o cabeçalho de versão
  `3.5` + 12 bytes zero antes do JSON; pushes de estado (8) também chegam com ele.
- Negociação: cliente manda `local_nonce` (cmd 3); lâmpada responde `remote_nonce` + HMAC-SHA256(local_key,
  local_nonce) (cmd 4); cliente manda HMAC-SHA256(local_key, remote_nonce) (cmd 5). Chave de sessão =
  ciphertext (16 bytes, sem IV e sem tag) de AES-GCM(local_key, IV = local_nonce[..12]) sobre
  `local_nonce XOR remote_nonce`. Essas três mensagens usam a local key; as seguintes, a chave de sessão.
- Payloads: consulta (16) = `{}`; comando (13) = `{"protocol":5,"t":<epoch>,"data":{"dps":{...}}}`;
  heartbeat (9) = `{"gwId":<id>,"devId":<id>}`. JSON sem espaços.
- A lâmpada numera as respostas com um seqno global dela, não com o seqno do pedido.
- Descoberta: lâmpadas anunciam por UDP nas portas 6666 (3.1, texto), 6667 (3.3+, cifrado com a chave
  UDP = md5(`yGAdlopoPVldABfn`)); para lâmpadas 3.5 é preciso mandar um pedido (cmd `0x25`,
  `{"from":"app","ip":<meu ip>}`) em broadcast para a porta 7000.
