"""Independent cross-validation of mixfile-vectors.json using python-cryptography (OpenSSL AES-GCM).
Confirms: layout iv||ct||tag, tag length 12B, base62, sha256 chain PRNG reproduction."""
import hashlib, json, sys
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

vectors = json.load(open("mixfile-vectors.json", encoding="utf-8"))["vectors"]
by_id = {v["id"]: v for v in vectors}
fails = 0

def check(ok, what):
    global fails
    print(("PASS " if ok else "FAIL ") + what)
    if not ok:
        fails += 1

def b62encode(data: bytes) -> str:
    alphabet = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"
    z = 0
    while z < len(data) and data[z] == 0:
        z += 1
    n = int.from_bytes(data[z:], "big")
    out = "0" if n == 0 and z < len(data) else ""
    digits = []
    while n > 0:
        digits.append(alphabet[n % 62])
        n //= 62
    return "0" * z + "".join(reversed(digits))

def b62decode(s: str) -> bytes:
    alphabet = {c: i for i, c in enumerate("0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ")}
    z = 0
    while z < len(s) and s[z] == "0":
        z += 1
    n = 0
    for c in s[z:]:
        n = n * 62 + alphabet[c]
    payload = b"" if n == 0 and z < len(s) else n.to_bytes((n.bit_length() + 7) // 8, "big")
    return b"\x00" * z + payload

def prng(seed: bytes, length: int) -> bytes:
    out, counter = b"", 0
    while len(out) < length:
        out += hashlib.sha256(seed + counter.to_bytes(4, "big")).digest()
        counter += 1
    return out[:length]

def enc(key: bytes, iv: bytes, pt: bytes) -> bytes:
    # AESGCM.encrypt returns ct||tag; tag size = len(key block) 16 by default -> set via AESGCM(key) then manual split
    full = AESGCM(key).encrypt(iv, pt, None)  # 128-bit tag
    return iv + full  # we compare only the first len(iv+pt) bytes and the truncated tag separately

# --- V1 ---
v1 = by_id["V1"]
key = bytes.fromhex(v1["key_hex"]); iv = bytes.fromhex(v1["iv_hex"])
pt = bytes.fromhex(v1["plaintext_hex"])
ct_expected = bytes.fromhex(v1["ciphertext_hex"])
full = AESGCM(key).encrypt(iv, pt, None)  # ct || tag(16B)
ours = iv + full[: len(pt)] + full[len(pt): len(pt) + 12]  # truncate tag to 12B
check(ours == ct_expected, "V1 ciphertext byte-exact vs OpenSSL AES-256-GCM (tag truncated to 12B)")
check(b62encode(hashlib.sha256(pt).digest()) == v1["plaintext_sha256_base62"], "V1 base62(sha256(pt)) matches")

# --- V2 ---
v2 = by_id["V2"]
key = bytes.fromhex(v2["key_hex"])
pt_all = prng(v2["prng_seed"].encode(), v2["plaintext_len"])
c0 = pt_all[: v2["chunk_size"]]
c1 = pt_all[v2["chunk_size"]:]
check(hashlib.sha256(c0).hexdigest() == v2["chunk0"]["pt_sha256_hex"], "V2 chunk0 plaintext hash (PRNG reproduction)")
check(hashlib.sha256(c1).hexdigest() == v2["chunk1"]["pt_sha256_hex"], "V2 chunk1 plaintext hash")
e0 = AESGCM(key).encrypt(bytes.fromhex(v2["chunk0"]["iv_hex"]), c0, None)
wire0 = bytes.fromhex(v2["chunk0"]["iv_hex"]) + e0[: len(c0)] + e0[len(c0): len(c0) + 12]  # iv||ct||tag12
check(hashlib.sha256(wire0).hexdigest() == v2["chunk0"]["ct_sha256_hex"], "V2 chunk0 ciphertext sha256 (wire iv||ct||tag12)")
check(wire0[:32].hex() == v2["chunk0"]["ct_first32_hex"], "V2 chunk0 first 32B (iv + ct[0:20])")
check(wire0[-32:].hex() == v2["chunk0"]["ct_last32_hex"], "V2 chunk0 last 32B (ct[-20:] + tag12)")
e1 = AESGCM(key).encrypt(bytes.fromhex(v2["chunk1"]["iv_hex"]), c1, None)
wire1 = bytes.fromhex(v2["chunk1"]["iv_hex"]) + e1[: len(c1)] + e1[len(c1): len(c1) + 12]
check(hashlib.sha256(wire1).hexdigest() == v2["chunk1"]["ct_sha256_hex"], "V2 chunk1 ciphertext sha256 (wire iv||ct||tag12)")
check(wire1[:32].hex() == v2["chunk1"]["ct_first32_hex"], "V2 chunk1 first 32B")
# index encryption
import gzip, io
gz = gzip.decompress(bytes.fromhex(v2["index_gzip_hex"]))
check(gz.decode() == v2["index_json"], "V2 index gzip decompresses to recorded JSON")
enc_full = AESGCM(key).encrypt(bytes.fromhex(v2["index_encrypted_iv_hex"]), bytes.fromhex(v2["index_gzip_hex"]), None)
iv3 = bytes.fromhex(v2["index_encrypted_iv_hex"])
gz_len = len(bytes.fromhex(v2["index_gzip_hex"]))
check((iv3 + enc_full[:gz_len] + enc_full[gz_len: gz_len + 12]).hex() == v2["index_encrypted_hex"], "V2 index ciphertext byte-exact (tag truncated to 12B)")

# --- V3 ---
v3 = by_id["V3"]
pw = hashlib.md5(b"123").digest()
check(pw.hex() == v3["key_hex"], "V3 key == MD5('123')")
share_bytes = b62decode(v3["share_code"])
# OpenSSL-backed lib cannot verify a 12B-truncated tag directly (GCM tags of length t
# are simply the first t bits of the 128-bit tag, so byte comparison is sufficient)
print("note: V3 compared by ciphertext bytes; JCE decrypt already verified in VectorVerify")
enc3 = AESGCM(pw).encrypt(bytes.fromhex(v3["iv_hex"]), v3["plaintext_utf8"].encode(), None)
ours3 = bytes.fromhex(v3["iv_hex"]) + enc3[: len(v3["plaintext_utf8"].encode())] + enc3[len(v3["plaintext_utf8"].encode()): len(v3["plaintext_utf8"].encode()) + 12]
check(ours3.hex() == v3["ciphertext_hex"], "V3 ciphertext byte-exact (AES-128 key, 12B tag)")
check(b62encode(ours3) == v3["share_code"], "V3 share_code base62 reproduction")

# --- V4 ---
v4 = by_id["V4"]
enc4 = AESGCM(bytes.fromhex(v4["key_hex"])).encrypt(bytes.fromhex(v4["iv_hex"]), b"", None)
check((bytes.fromhex(v4["iv_hex"]) + enc4[:12]).hex() == v4["ciphertext_hex"], "V4 empty plaintext: iv || 12B tag")

print("FAILURES:", fails)
sys.exit(1 if fails else 0)
