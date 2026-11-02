# Vetores de teste da API de nuvem da Tuya

Gera `crates/tuya-cloud/tests/fixtures/cloud_vectors.json` usando o
[tuya-device-sharing-sdk](https://github.com/tuya/tuya-device-sharing-sdk) (versão fixada em
`requirements.txt`, o mesmo SDK da integração Tuya do Home Assistant) como referência. Os testes do
`elgin-tuya-cloud` comparam a implementação em Rust com esses valores.

O gerador fixa o `requestId`, o nonce e o horário, e usa tokens falsos: a saída é determinística e
nenhuma rede é acessada.

```sh
py -3.11 -m venv .venv-cloud-vectors
.venv-cloud-vectors/Scripts/python -m pip install -r tools/tuya-cloud-vectors/requirements.txt
.venv-cloud-vectors/Scripts/python -I tools/tuya-cloud-vectors/generate.py crates/tuya-cloud/tests/fixtures/cloud_vectors.json
```

O que é conferido (funções do `tuya_sharing/customerapi.py`): `hash_key` (md5 de requestId +
refresh token), `secret` (`_secret_generating`), parâmetros cifrados (`_aes_gcm_encrypt`: dois base64
concatenados, o do nonce e o do ciphertext), assinatura `X-sign` (`_restful_sign`) e a decifragem do
`result` da resposta (`_aex_gcm_decrypt`: base64 de nonce + ciphertext + tag).
