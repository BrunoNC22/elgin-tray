"""Gera vetores de teste da API de nuvem da Tuya usando o próprio tuya-device-sharing-sdk.

Fixa o requestId, o nonce do AES-GCM e o horário, e grava em JSON tudo que o cliente Rust precisa
reproduzir: chave derivada (secret), assinatura, parâmetros cifrados e uma resposta cifrada com o
JSON decifrado. Tokens e ids são falsos. Nenhuma rede é acessada.

Uso (na raiz do repositório, Python nativo do Windows):
    py -3.11 -m venv .venv-cloud-vectors
    .venv-cloud-vectors/Scripts/python -m pip install -r tools/tuya-cloud-vectors/requirements.txt
    .venv-cloud-vectors/Scripts/python -I tools/tuya-cloud-vectors/generate.py crates/tuya-cloud/tests/fixtures/cloud_vectors.json
"""

import base64
import hashlib
import json
import sys

from tuya_sharing import customerapi as api

CLIENT_ID = "HA_3y9q4ak7g4ephrvke"
REFRESH_TOKEN = "renovacao0000000000000000000000x"
ACCESS_TOKEN = "acesso00000000000000000000000000"
REQUEST_ID = "0f1e2d3c-4b5a-4968-8776-a5b4c3d2e1f0"
NONCE = "ABCDEFGHJKMN"
TIME_MS = "1700000000000"


def main():
    out = sys.argv[1]
    api._random_nonce = lambda e=32: NONCE  # nonce fixo para o vetor ser determinístico

    hash_key = hashlib.md5((REQUEST_ID + REFRESH_TOKEN).encode()).hexdigest()
    secret = api._secret_generating(REQUEST_ID, "", hash_key)

    params = {"homeId": "123456"}
    params_json = api._form_to_json(params)
    query_encdata = str(api._aes_gcm_encrypt(params_json, secret), encoding="utf8")

    headers = {"X-appKey": CLIENT_ID, "X-requestId": REQUEST_ID, "X-sid": "", "X-time": TIME_MS,
               "X-token": ACCESS_TOKEN}
    sign_with_query = api._restful_sign(hash_key, query_encdata, "", headers)
    sign_without_query = api._restful_sign(hash_key, "", "", headers)

    # Uma resposta como a do servidor: base64(nonce + ciphertext + tag), cifrada com o secret.
    result_json = json.dumps([{"id": "bf00000000000000000000", "name": "Lâmpada", "local_key": "0123456789abcdef",
                               "category": "dj", "product_name": "10W Smart color"}], ensure_ascii=False)
    encrypted = api._aes_gcm_encrypt(result_json, secret)
    # _aes_gcm_encrypt devolve base64(nonce) + base64(ciphertext); o servidor manda base64(nonce + ciphertext).
    nonce_and_ciphertext = base64.b64decode(encrypted[:16]) + base64.b64decode(encrypted[16:])
    response_result = base64.b64encode(nonce_and_ciphertext).decode()
    assert api._aex_gcm_decrypt(response_result, secret) == result_json

    vectors = {
        "generator": "tools/tuya-cloud-vectors/generate.py",
        "client_id": CLIENT_ID,
        "request_id": REQUEST_ID,
        "refresh_token": REFRESH_TOKEN,
        "access_token": ACCESS_TOKEN,
        "time_ms": TIME_MS,
        "nonce": NONCE,
        "hash_key": hash_key,
        "secret": secret,
        "params_json": params_json,
        "query_encdata": query_encdata,
        "sign_with_query": sign_with_query,
        "sign_without_query": sign_without_query,
        "response_result": response_result,
        "response_json": result_json,
    }
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        json.dump(vectors, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"vetores gravados em {out}")


if __name__ == "__main__":
    main()
