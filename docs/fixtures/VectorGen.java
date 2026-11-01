import javax.crypto.Cipher;
import javax.crypto.spec.GCMParameterSpec;
import javax.crypto.spec.SecretKeySpec;
import java.io.ByteArrayOutputStream;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.zip.GZIPOutputStream;

/**
 * Generates MixFile-compatible crypto test vectors for HeartShard.
 *
 * Replicates mixfile-core AES.kt exactly:
 *   Cipher "AES/GCM/NoPadding", GCMParameterSpec(96, iv), no AAD,
 *   output layout = iv(12B) || ciphertext || tag(12B).
 *
 * Base62 mirrors BigIntBaseN with alphabet 0-9a-zA-Z, leading 0x00 bytes -> leading '0'.
 */
public class VectorGen {
    static final String B62 = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

    static byte[] sha256(byte[] data) throws Exception {
        return MessageDigest.getInstance("SHA-256").digest(data);
    }

    static byte[] md5(byte[] data) throws Exception {
        return MessageDigest.getInstance("MD5").digest(data);
    }

    // AES.kt:39-45 equivalent. tag = 96 bits.
    static byte[] encryptAES(byte[] data, byte[] key, byte[] iv) throws Exception {
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.ENCRYPT_MODE, new SecretKeySpec(key, "AES"), new GCMParameterSpec(96, iv));
        byte[] ct = cipher.doFinal(data);
        byte[] out = new byte[iv.length + ct.length];
        System.arraycopy(iv, 0, out, 0, iv.length);
        System.arraycopy(ct, 0, out, iv.length, ct.length);
        return out;
    }

    static String base62(byte[] data) {
        if (data.length == 0) return "";
        int z = 0;
        while (z < data.length && data[z] == 0) z++;
        StringBuilder sb = new StringBuilder();
        if (z < data.length) {
            BigInteger n = new BigInteger(1, Arrays.copyOfRange(data, z, data.length));
            BigInteger r = BigInteger.valueOf(62);
            if (n.signum() == 0) sb.append('0');
            while (n.signum() != 0) {
                sb.insert(0, B62.charAt(n.mod(r).intValue()));
                n = n.divide(r);
            }
        }
        for (int i = 0; i < z; i++) sb.insert(0, '0');
        return sb.toString();
    }

    static byte[] gzip(byte[] data) throws Exception {
        ByteArrayOutputStream bos = new ByteArrayOutputStream();
        GZIPOutputStream gz = new GZIPOutputStream(bos);
        gz.write(data);
        gz.close();
        return bos.toByteArray();
    }

    // Deterministic pseudo-random bytes: SHA256(seed || counter32BE) chained.
    static byte[] prng(byte[] seed, int len) throws Exception {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        int counter = 0;
        while (out.size() < len) {
            ByteArrayOutputStream b = new ByteArrayOutputStream();
            b.write(seed);
            b.write((counter >>> 24) & 0xFF);
            b.write((counter >>> 16) & 0xFF);
            b.write((counter >>> 8) & 0xFF);
            b.write(counter & 0xFF);
            out.write(sha256(b.toByteArray()));
            counter++;
        }
        return Arrays.copyOf(out.toByteArray(), len);
    }

    static String hex(byte[] b) {
        StringBuilder sb = new StringBuilder(b.length * 2);
        for (byte x : b) sb.append(String.format("%02x", x));
        return sb.toString();
    }

    static String jsonStr(String s) {
        StringBuilder sb = new StringBuilder("\"");
        for (char c : s.toCharArray()) {
            if (c == '"' || c == '\\') sb.append('\\').append(c);
            else if (c < 0x20) sb.append(String.format("\\u%04x", (int) c));
            else sb.append(c);
        }
        return sb.append('"').toString();
    }

    public static void main(String[] args) throws Exception {
        Path outDir = Paths.get(args.length > 0 ? args[0] : ".");
        Files.createDirectories(outDir);

        // Fixed keys / IVs (hex-documented in the study doc)
        byte[] key32 = new byte[32];
        for (int i = 0; i < 32; i++) key32[i] = (byte) i;                 // 000102..1f
        byte[] ivA = new byte[12];
        for (int i = 0; i < 12; i++) ivA[i] = (byte) (0xa0 + i);          // a0..ab
        byte[] ivB = new byte[12];
        for (int i = 0; i < 12; i++) ivB[i] = (byte) (0xb0 + i);          // b0..bb
        byte[] ivC = new byte[12];
        for (int i = 0; i < 12; i++) ivC[i] = (byte) (0xc0 + i);          // c0..cb

        StringBuilder json = new StringBuilder();
        json.append("{\n  \"generator\": \"VectorGen.java (JCE AES/GCM/NoPadding, tag 96bit, layout iv||ct||tag)\",\n");
        json.append("  \"base62_alphabet\": \"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\",\n");
        json.append("  \"vectors\": [\n");

        // ---------- V1: 64-byte plaintext, AES-256, full ciphertext ----------
        byte[] pt1 = new byte[64];
        byte[] tpl = "HeartShard M0 vector 01: MixFile AES-GCM alignment.".getBytes(StandardCharsets.US_ASCII);
        for (int i = 0; i < 64; i++) pt1[i] = tpl[i % tpl.length];
        byte[] ct1 = encryptAES(pt1, key32, ivA);
        byte[] pt1h = sha256(pt1);
        String v1 = String.format(
            "    {\n" +
            "      \"id\": \"V1\",\n" +
            "      \"desc\": \"64B plaintext, AES-256 key, fixed iv, full ciphertext\",\n" +
            "      \"key_len\": 32, \"key_hex\": \"%s\", \"iv_hex\": \"%s\",\n" +
            "      \"plaintext_len\": 64, \"plaintext_hex\": \"%s\",\n" +
            "      \"plaintext_sha256_hex\": \"%s\", \"plaintext_sha256_base62\": \"%s\",\n" +
            "      \"ciphertext_hex\": \"%s\", \"ciphertext_sha256_hex\": \"%s\"\n" +
            "    }",
            hex(key32), hex(ivA), hex(pt1), hex(pt1h), base62(pt1h), hex(ct1), hex(sha256(ct1)));
        json.append(v1);

        // ---------- V2: 1.5 MiB pseudo-random plaintext, 1 MiB chunks ----------
        byte[] pt2 = prng("heartshard-m0-prng-v2".getBytes(StandardCharsets.US_ASCII), 1572864);
        byte[] c0 = Arrays.copyOfRange(pt2, 0, 1048576);
        byte[] c1 = Arrays.copyOfRange(pt2, 1048576, 1572864);
        byte[] e0 = encryptAES(c0, key32, ivA);
        byte[] e1 = encryptAES(c1, key32, ivB);
        byte[] h0 = sha256(c0), h1 = sha256(c1);
        String url0 = "https://example.com/blob/c0#" + base62(h0);
        String url1 = "https://example.com/blob/c1#" + base62(h1);
        // MixFile JSON, kotlinx.serialization field order (declaration order, no spaces)
        String indexJson = String.format(
            "{\"chunk_size\":1048576,\"file_size\":1572864,\"version\":0,\"file_list\":[%s,%s]}",
            jsonStr(url0), jsonStr(url1));
        byte[] indexGz = gzip(indexJson.getBytes(StandardCharsets.UTF_8));
        byte[] indexEnc = encryptAES(indexGz, key32, ivC);

        String v2 = String.format(
            ",\n    {\n" +
            "      \"id\": \"V2\",\n" +
            "      \"desc\": \"1.5MiB deterministic PRNG plaintext, chunk_size=1MiB, 2 chunks; index JSON -> gzip -> AES; boundary+hash only (full bytes via prng seed)\",\n" +
            "      \"key_len\": 32, \"key_hex\": \"%s\",\n" +
            "      \"prng_seed\": \"heartshard-m0-prng-v2\", \"plaintext_len\": 1572864,\n" +
            "      \"chunk_size\": 1048576,\n" +
            "      \"chunk0\": {\"iv_hex\": \"%s\", \"pt_len\": 1048576, \"pt_sha256_hex\": \"%s\", \"pt_sha256_base62\": \"%s\", \"ct_len\": %d, \"ct_sha256_hex\": \"%s\", \"ct_first32_hex\": \"%s\", \"ct_last32_hex\": \"%s\"},\n" +
            "      \"chunk1\": {\"iv_hex\": \"%s\", \"pt_len\": 524288, \"pt_sha256_hex\": \"%s\", \"pt_sha256_base62\": \"%s\", \"ct_len\": %d, \"ct_sha256_hex\": \"%s\", \"ct_first32_hex\": \"%s\", \"ct_last32_hex\": \"%s\"},\n" +
            "      \"index_json\": %s,\n" +
            "      \"index_gzip_hex\": \"%s\",\n" +
            "      \"index_gzip_note\": \"Java GZIPOutputStream default header (MTIME=0, OS=0); other stacks may emit different but valid gzip - compare after decompression, not byte-for-byte\",\n" +
            "      \"index_encrypted_hex\": \"%s\",\n" +
            "      \"index_encrypted_iv_hex\": \"%s\",\n" +
            "      \"index_encrypted_sha256_hex\": \"%s\"\n" +
            "    }",
            hex(key32),
            hex(ivA), hex(h0), base62(h0), e0.length, hex(sha256(e0)), hex(Arrays.copyOf(e0, 32)), hex(Arrays.copyOfRange(e0, e0.length - 32, e0.length)),
            hex(ivB), hex(h1), base62(h1), e1.length, hex(sha256(e1)), hex(Arrays.copyOf(e1, 32)), hex(Arrays.copyOfRange(e1, e1.length - 32, e1.length)),
            jsonStr(indexJson),
            hex(indexGz),
            hex(indexEnc), hex(Arrays.copyOf(indexEnc, 12)), hex(sha256(indexEnc)));
        json.append(v2);

        // ---------- V3: share code (AES-128 via MD5("123") key), full base62 output ----------
        byte[] pw = md5("123".getBytes(StandardCharsets.US_ASCII));
        String shareJson = String.format(
            "{\"f\":\"sunset-beach.jpg\",\"s\":1572864,\"h\":0,\"u\":%s,\"k\":\"%s\",\"r\":\"\"}",
            jsonStr("https://example.com/blob/index#" + base62(sha256(indexGz))),
            base62(key32));
        byte[] shareEnc = encryptAES(shareJson.getBytes(StandardCharsets.UTF_8), pw, ivA);
        String shareCode = base62(shareEnc);

        String v3 = String.format(
            ",\n    {\n" +
            "      \"id\": \"V3\",\n" +
            "      \"desc\": \"MixShareInfo share code: JSON encrypted with key=MD5(\\\"123\\\") (16B => AES-128), base62-encoded; full output\",\n" +
            "      \"key_len\": 16, \"key_hex\": \"%s\", \"key_derivation\": \"MD5 of UTF-8 string \\\"123\\\"\",\n" +
            "      \"iv_hex\": \"%s\",\n" +
            "      \"plaintext_utf8\": %s,\n" +
            "      \"ciphertext_hex\": \"%s\",\n" +
            "      \"share_code\": \"%s\",\n" +
            "      \"ciphertext_sha256_hex\": \"%s\"\n" +
            "    }",
            hex(pw), hex(ivA), jsonStr(shareJson), hex(shareEnc), shareCode, hex(sha256(shareEnc)));
        json.append(v3);

        // ---------- V4: empty plaintext ----------
        byte[] ct4 = encryptAES(new byte[0], key32, ivA);
        String v4 = String.format(
            ",\n    {\n" +
            "      \"id\": \"V4\",\n" +
            "      \"desc\": \"empty plaintext, AES-256: output = iv || 12B tag only\",\n" +
            "      \"key_len\": 32, \"key_hex\": \"%s\", \"iv_hex\": \"%s\",\n" +
            "      \"plaintext_len\": 0, \"plaintext_hex\": \"\",\n" +
            "      \"ciphertext_hex\": \"%s\", \"ciphertext_len\": %d\n" +
            "    }",
            hex(key32), hex(ivA), hex(ct4), ct4.length);
        json.append(v4);

        json.append("\n  ]\n}\n");

        Files.write(outDir.resolve("mixfile-vectors.json"), json.toString().getBytes(StandardCharsets.UTF_8));

        // Human-readable summary
        System.out.println("== V1 ==");
        System.out.println("ct(" + ct1.length + "B): " + hex(ct1));
        System.out.println("pt_sha256_b62: " + base62(pt1h));
        System.out.println("== V2 ==");
        System.out.println("chunk0 ct sha256: " + hex(sha256(e0)) + " len=" + e0.length);
        System.out.println("chunk1 ct sha256: " + hex(sha256(e1)) + " len=" + e1.length);
        System.out.println("index_json: " + indexJson);
        System.out.println("index_gz(" + indexGz.length + "B): " + hex(indexGz));
        System.out.println("index_enc(" + indexEnc.length + "B): " + hex(indexEnc));
        System.out.println("== V3 ==");
        System.out.println("share_json: " + shareJson);
        System.out.println("md5_123_key: " + hex(pw));
        System.out.println("share_code(" + shareCode.length() + "): " + shareCode);
        System.out.println("== V4 ==");
        System.out.println("ct(" + ct4.length + "B): " + hex(ct4));
        System.out.println("Wrote: " + outDir.resolve("mixfile-vectors.json").toAbsolutePath());
    }
}
