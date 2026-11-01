import javax.crypto.Cipher;
import javax.crypto.spec.GCMParameterSpec;
import javax.crypto.spec.SecretKeySpec;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.security.MessageDigest;
import java.util.Arrays;

/** Round-trip verification of the generated vectors (decrypt side). */
public class VectorVerify {
    static final String B62 = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

    static byte[] decryptAES(byte[] data, byte[] key) throws Exception {
        byte[] iv = Arrays.copyOf(data, 12);
        byte[] ct = Arrays.copyOfRange(data, 12, data.length);
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.DECRYPT_MODE, new SecretKeySpec(key, "AES"), new GCMParameterSpec(96, iv));
        return cipher.doFinal(ct);
    }

    // Mirrors BigIntBaseN: leading '0' chars = leading 0x00 bytes; digits via big-int
    static byte[] base62Decode(String s) {
        int z = 0;
        while (z < s.length() && s.charAt(z) == '0') z++;
        BigInteger n = BigInteger.ZERO;
        BigInteger r = BigInteger.valueOf(62);
        for (int i = z; i < s.length(); i++) {
            int d = B62.indexOf(s.charAt(i));
            if (d < 0) throw new RuntimeException("bad base62 char: " + s.charAt(i));
            n = n.multiply(r).add(BigInteger.valueOf(d));
        }
        byte[] payload = n.toByteArray(); // may carry a leading 0x00 sign byte
        int start = (payload.length > 1 && payload[0] == 0) ? 1 : 0;
        byte[] out = new byte[z + (payload.length - start)];
        System.arraycopy(payload, start, out, z, payload.length - start);
        return out;
    }

    static void check(boolean ok, String what) {
        System.out.println((ok ? "PASS " : "FAIL ") + what);
        if (!ok) System.exit(1);
    }

    static byte[] hex(String h) {
        byte[] out = new byte[h.length() / 2];
        for (int i = 0; i < out.length; i++) {
            out[i] = (byte) Integer.parseInt(h.substring(i * 2, i * 2 + 2), 16);
        }
        return out;
    }

    static String extract(String json, String anchor, String field) {
        int a = json.indexOf(anchor);
        if (a < 0) throw new RuntimeException("anchor not found: " + anchor);
        int f = json.indexOf("\"" + field + "\"", a);
        if (f < 0) throw new RuntimeException("field not found: " + field);
        int colon = json.indexOf(':', f + field.length() + 2);
        int start = json.indexOf('"', colon) + 1;
        int end = json.indexOf('"', start);
        return json.substring(start, end);
    }

    public static void main(String[] args) throws Exception {
        byte[] key32 = new byte[32];
        for (int i = 0; i < 32; i++) key32[i] = (byte) i;
        byte[] pw = MessageDigest.getInstance("MD5").digest("123".getBytes(StandardCharsets.US_ASCII));

        String json = new String(Files.readAllBytes(Paths.get("mixfile-vectors.json")), StandardCharsets.UTF_8);

        // V1
        String ct1 = extract(json, "\"id\": \"V1\"", "ciphertext_hex");
        byte[] pt = decryptAES(hex(ct1), key32);
        check(new String(pt, StandardCharsets.US_ASCII).startsWith("HeartShard M0 vector 01"), "V1 decrypt round-trip");
        String ptB62 = extract(json, "\"id\": \"V1\"", "plaintext_sha256_base62");
        check(Arrays.equals(base62Decode(ptB62), MessageDigest.getInstance("SHA-256").digest(pt)),
                "V1 base62(sha256(plaintext)) round-trip");

        // V2 index
        String indexEnc = extract(json, "\"id\": \"V2\"", "index_encrypted_hex");
        byte[] indexGz = decryptAES(hex(indexEnc), key32);
        String indexGzHex = extract(json, "\"id\": \"V2\"", "index_gzip_hex");
        check(Arrays.equals(indexGz, hex(indexGzHex)), "V2 index decrypt == recorded gzip bytes");
        check(extract(json, "chunk0", "ct_sha256_hex").length() == 64, "V2 chunk0 hash field present");
        check(extract(json, "chunk1", "ct_first32_hex").length() == 64, "V2 chunk1 boundary field present");

        // V3 share code
        String code = extract(json, "\"id\": \"V3\"", "share_code");
        byte[] shareJsonBytes = decryptAES(base62Decode(code), pw);
        String shareJson = new String(shareJsonBytes, StandardCharsets.UTF_8);
        check(shareJson.startsWith("{\"f\":\"sunset-beach.jpg\",\"s\":1572864,\"h\":0,"),
                "V3 share code decrypts to MixShareInfo JSON (declaration field order)");
        check(shareJson.contains("\"k\":\"03AuLtjc7TJLctqJ2Unu3mfAGcxg9lrkrCWgKbidLF\""),
                "V3 share code carries base62 key");

        // V4
        byte[] empty = decryptAES(hex(extract(json, "\"id\": \"V4\"", "ciphertext_hex")), key32);
        check(empty.length == 0, "V4 empty plaintext round-trip");

        // key base62 round-trip (leading zero byte preserved as '0')
        check(Arrays.equals(base62Decode("03AuLtjc7TJLctqJ2Unu3mfAGcxg9lrkrCWgKbidLF"), key32),
                "key base62 round-trip (leading zero byte)");

        System.out.println("ALL VECTOR ROUND-TRIP CHECKS PASSED");
    }
}
