"""Gera vetores de teste do protocolo Tuya 3.5 usando o próprio tinytuya como referência.

Tudo usa chave falsa e nonces/IVs fixos, então a saída é determinística (rodar de novo gera o mesmo
arquivo). Nenhuma rede é acessada.

Uso (na raiz do repositório):
    py -3.11 -m venv .venv-vectors
    .venv-vectors/Scripts/python -m pip install -r tools/tuya-vectors/requirements.txt
    .venv-vectors/Scripts/python -I tools/tuya-vectors/generate.py crates/tuya-lan/tests/fixtures/vectors.json
"""

import hashlib
import hmac
import json
import sys

import tinytuya
from tinytuya.core import command_types as CT
from tinytuya.core import header as H
from tinytuya.core.message_helper import TuyaMessage, pack_message, unpack_message
from tinytuya.core.udp_helper import decrypt_udp, encrypt as udp_encrypt, udpkey

LOCAL_KEY = b"0123456789abcdef"
DEVICE_ID = "bf0123456789abcdefgh"
LOCAL_NONCE = bytes(range(0x10, 0x20))
REMOTE_NONCE = bytes(range(0xA0, 0xB0))
VERSION_HEADER = H.PROTOCOL_35_HEADER  # b"3.5" + 12 bytes zero


def iv(n):
    """IV fixo de 12 bytes, diferente para cada vetor."""
    return bytes([n] * 12)


def hexs(data):
    return data.hex()


def json_bytes(obj):
    # Igual ao tinytuya: json.dumps sem espaços.
    return json.dumps(obj).replace(" ", "").encode()


def frame(name, key, seqno, cmd, payload, iv_bytes, retcode=None, note=""):
    """Empacota um quadro 6699 e confere que o tinytuya desempacota de volta."""
    msg = TuyaMessage(seqno, cmd, retcode, payload, 0, True, H.PREFIX_6699_VALUE, iv_bytes)
    data = pack_message(msg, hmac_key=key)
    unpacked = unpack_message(data, hmac_key=key, no_retcode=(retcode is None))
    assert unpacked.crc_good, name
    assert unpacked.payload == payload, name
    if retcode is not None:
        assert unpacked.retcode == retcode, name
    # O pack_message põe o retcode (quando há) dentro da criptografia, antes do payload.
    plaintext = (retcode.to_bytes(4, "big") if retcode is not None else b"") + payload
    return {
        "name": name,
        "note": note,
        "key": hexs(key),
        "seqno": seqno,
        "cmd": cmd,
        "retcode": retcode,
        "iv": hexs(iv_bytes),
        "payload": hexs(payload),
        "plaintext": hexs(plaintext),
        "frame": hexs(data),
    }


def session_vectors():
    """Negociação de chave usando os métodos do XenonDevice do tinytuya."""
    device = tinytuya.Device(DEVICE_ID, "127.0.0.1", LOCAL_KEY.decode(), version=3.5)
    device.local_nonce = LOCAL_NONCE
    device.remote_nonce = b""
    device.local_key = device.real_local_key

    hmac_of_local = hmac.new(LOCAL_KEY, LOCAL_NONCE, hashlib.sha256).digest()
    response = TuyaMessage(1, CT.SESS_KEY_NEG_RESP, 0, REMOTE_NONCE + hmac_of_local, 0, True, H.PREFIX_6699_VALUE, None)
    step3 = device._negotiate_session_key_generate_step_3(response)
    assert step3 and step3.cmd == CT.SESS_KEY_NEG_FINISH
    device._negotiate_session_key_generate_finalize()
    session_key = device.local_key

    hmac_of_remote = hmac.new(LOCAL_KEY, REMOTE_NONCE, hashlib.sha256).digest()
    assert step3.payload == hmac_of_remote

    frames = [
        frame("neg_start", LOCAL_KEY, 1, CT.SESS_KEY_NEG_START, LOCAL_NONCE, iv(0x01),
              note="cliente -> lâmpada: local_nonce, cifrado com a local key"),
        frame("neg_resp", LOCAL_KEY, 1, CT.SESS_KEY_NEG_RESP, REMOTE_NONCE + hmac_of_local, iv(0x02), retcode=0,
              note="lâmpada -> cliente: remote_nonce + HMAC(local_key, local_nonce), com retcode"),
        frame("neg_finish", LOCAL_KEY, 2, CT.SESS_KEY_NEG_FINISH, hmac_of_remote, iv(0x03),
              note="cliente -> lâmpada: HMAC(local_key, remote_nonce)"),
    ]
    return {
        "local_key": hexs(LOCAL_KEY),
        "local_nonce": hexs(LOCAL_NONCE),
        "remote_nonce": hexs(REMOTE_NONCE),
        "hmac_of_local_nonce": hexs(hmac_of_local),
        "hmac_of_remote_nonce": hexs(hmac_of_remote),
        "session_key": hexs(session_key),
        "frames": frames,
    }, session_key


def session_frames(session_key):
    """Mensagens depois da negociação, cifradas com a chave de sessão."""
    control = {"protocol": 5, "t": 1700000000, "data": {"dps": {"20": True, "21": "colour", "24": "00f003e803e8"}}}
    status = {"dps": {"20": True, "21": "white", "22": 1000, "23": 1000, "24": "000003e803e8", "26": 0}}
    push = {"protocol": 4, "t": 1700000001, "data": {"dps": {"22": 500}}}
    return [
        frame("dp_query_new", session_key, 3, CT.DP_QUERY_NEW, json_bytes({}), iv(0x04),
              note="consulta de estado: payload {} sem cabeçalho de versão"),
        frame("dp_query_new_resp", session_key, 7, CT.DP_QUERY_NEW, json_bytes(status), iv(0x05), retcode=0,
              note="resposta do estado (seqno da lâmpada é global, não o do pedido)"),
        frame("heart_beat", session_key, 4, CT.HEART_BEAT, json_bytes({"gwId": DEVICE_ID, "devId": DEVICE_ID}), iv(0x06),
              note="heartbeat sem cabeçalho de versão"),
        frame("heart_beat_resp", session_key, 8, CT.HEART_BEAT, b"", iv(0x07), retcode=0,
              note="resposta do heartbeat: só retcode"),
        frame("control_new", session_key, 5, CT.CONTROL_NEW, VERSION_HEADER + json_bytes(control), iv(0x08),
              note="comando: cabeçalho de versão '3.5'+12 zeros antes do JSON"),
        frame("control_new_ack", session_key, 9, CT.CONTROL_NEW, b"", iv(0x09), retcode=0,
              note="confirmação do comando: só retcode"),
        frame("status_push", session_key, 10, CT.STATUS, VERSION_HEADER + json_bytes(push), iv(0x0A), retcode=0,
              note="push de estado da lâmpada: retcode + cabeçalho de versão + JSON"),
    ]


def udp_vectors():
    broadcast_35 = {"ip": "192.168.0.50", "gwId": DEVICE_ID, "active": 2, "ability": 0, "mode": 0, "encrypt": True,
                    "productKey": "keyfakeproduct00", "version": "3.5", "token": True, "wf_cfg": True}
    broadcast_33 = dict(broadcast_35, version="3.3")
    payload_35 = json_bytes(broadcast_35)
    payload_33 = json_bytes(broadcast_33)

    vectors = []
    for name, retcode in (("broadcast_35_with_retcode", 0), ("broadcast_35_without_retcode", None)):
        msg = TuyaMessage(0, CT.BOARDCAST_LPV34, retcode, payload_35, 0, True, H.PREFIX_6699_VALUE, iv(0x20 + len(vectors)))
        data = pack_message(msg, hmac_key=udpkey)
        assert json.loads(decrypt_udp(data)) == broadcast_35, name
        vectors.append({"name": name, "datagram": hexs(data), "json": broadcast_35})

    # No formato 55AA o pack_message não escreve o retcode, mas quem recebe espera 4 bytes dele
    # antes do payload (como num broadcast real).
    encrypted = udp_encrypt(payload_33, udpkey)
    msg = TuyaMessage(0, CT.UDP_NEW, 0, b"\x00\x00\x00\x00" + encrypted, 0, True, H.PREFIX_55AA_VALUE, False)
    data = pack_message(msg)
    assert json.loads(decrypt_udp(data)) == broadcast_33
    vectors.append({"name": "broadcast_33_55aa", "datagram": hexs(data), "json": broadcast_33})

    request_payload = json_bytes({"from": "app", "ip": "192.168.0.10"})
    request = TuyaMessage(0, CT.REQ_DEVINFO, None, request_payload, 0, True, H.PREFIX_6699_VALUE, iv(0x30))
    return {
        "udp_key": hexs(udpkey),
        "broadcasts": vectors,
        "discovery_request": {
            "note": "enviado em broadcast para a porta 7000 para lâmpadas 3.5 se anunciarem",
            "iv": hexs(iv(0x30)),
            "plaintext": hexs(request_payload),
            "datagram": hexs(pack_message(request, hmac_key=udpkey)),
        },
    }


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "vectors.json"
    session, session_key = session_vectors()
    vectors = {
        "generator": "tools/tuya-vectors/generate.py",
        "tinytuya_version": tinytuya.__version__,
        "device_id": DEVICE_ID,
        "version_header": hexs(VERSION_HEADER),
        "session": session,
        "frames": session_frames(session_key),
        "udp": udp_vectors(),
    }
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        json.dump(vectors, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"vetores gravados em {out} (tinytuya {tinytuya.__version__})")


if __name__ == "__main__":
    main()
