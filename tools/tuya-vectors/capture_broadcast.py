"""Captura a resposta real de uma lâmpada Tuya ao pedido de descoberta e grava uma versão anonimizada.

Escuta as portas UDP 6666, 6667 e 7000, manda o pedido de descoberta (cmd 0x25) em broadcast para a
porta 7000 e guarda a primeira resposta. Antes de gravar, troca gwId, ip e uuid por valores falsos
de mesmo tamanho e cifra de novo com a mesma estrutura (comando, retcode, formatação do JSON). Assim
a fixture tem o formato real da lâmpada sem nenhum dado dela.

Uso (na raiz do repositório, com o venv do tools/tuya-vectors/README.md):
    .venv-vectors/Scripts/python -I tools/tuya-vectors/capture_broadcast.py crates/tuya-lan/tests/fixtures/real_broadcast.json
"""

import json
import select
import socket
import sys
import time

from tinytuya.core import header as H
from tinytuya.core.message_helper import TuyaMessage, pack_message, parse_header, unpack_message
from tinytuya.core.udp_helper import decrypt_udp, udpkey

PORTS = (6666, 6667, 7000)
LISTEN_SECONDS = 14
FIXED_IV = bytes([0x77] * 12)


def fake_like(text, filler):
    """Valor falso com o mesmo tamanho do original."""
    return (filler * len(text))[: len(text)]


def fake_ip(_ip):
    return "192.168.0.50"


def main():
    out = sys.argv[1]
    sockets = []
    for port in PORTS:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_UDP)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
        s.bind(("", port))
        sockets.append((port, s))
    sender = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sender.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    request = pack_message(
        TuyaMessage(0, 0x25, None, b'{"from":"app","ip":"0.0.0.0"}', 0, True, H.PREFIX_6699_VALUE, True),
        hmac_key=udpkey,
    )

    deadline = time.time() + LISTEN_SECONDS
    last_request = 0.0
    while time.time() < deadline:
        if time.time() - last_request > 2:
            sender.sendto(request, ("255.255.255.255", 7000))
            last_request = time.time()
        ready, _, _ = select.select([s for _, s in sockets], [], [], 0.5)
        for s in ready:
            data, _ = s.recvfrom(4096)
            port = next(p for p, x in sockets if x is s)
            try:
                announced = json.loads(decrypt_udp(data))
            except Exception:
                continue
            if announced.get("from") == "app" or data[:4] != H.PREFIX_6699_BIN:
                continue
            header = parse_header(data)
            raw = unpack_message(data, hmac_key=udpkey, no_retcode=True).payload
            text = raw.decode("latin1")
            for field, filler in (("gwId", "bf"), ("uuid", "u0")):
                if field in announced:
                    text = text.replace(announced[field], fake_like(announced[field], filler))
            text = text.replace(announced["ip"], fake_ip(announced["ip"]))
            anonymized = text.encode("latin1")
            message = TuyaMessage(header.seqno, header.cmd, None, anonymized, 0, True, H.PREFIX_6699_VALUE, FIXED_IV)
            datagram = pack_message(message, hmac_key=udpkey)
            fixture = {
                "note": "resposta real de uma lâmpada Elgin (Tuya 3.5) ao pedido de descoberta, anonimizada",
                "port": port,
                "cmd": header.cmd,
                "datagram": datagram.hex(),
                "json": json.loads(decrypt_udp(datagram)),
            }
            with open(out, "w", encoding="utf-8", newline="\n") as f:
                json.dump(fixture, f, indent=2, ensure_ascii=False)
                f.write("\n")
            print(f"capturado na porta {port}, cmd {header.cmd}; gravado em {out}")
            return
    print("nenhuma lâmpada respondeu")
    sys.exit(1)


if __name__ == "__main__":
    main()
