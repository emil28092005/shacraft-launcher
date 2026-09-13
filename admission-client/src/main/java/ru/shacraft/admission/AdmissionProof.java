package ru.shacraft.admission;

import java.nio.charset.StandardCharsets;
import java.security.KeyFactory;
import java.security.PrivateKey;
import java.security.Signature;
import java.security.spec.PKCS8EncodedKeySpec;
import java.util.Base64;
import java.util.regex.Pattern;

/** The only client credential is an ephemeral private key supplied by the launcher. */
public final class AdmissionProof {
    public static final String SERVER_ID = "minigames";
    private static final Pattern OPAQUE = Pattern.compile("[A-Za-z0-9_-]{43}");
    private static final Pattern NICKNAME = Pattern.compile("[A-Za-z0-9_]{3,16}");

    private AdmissionProof() {}

    public static boolean validOpaque(String value) {
        return value != null && OPAQUE.matcher(value).matches();
    }

    public static byte[] transcript(String ticket, String serverId, String nickname, String nonce) {
        if (!validOpaque(ticket) || !validOpaque(nonce) || !SERVER_ID.equals(serverId)
                || nickname == null || !NICKNAME.matcher(nickname).matches()) {
            throw new IllegalArgumentException("Invalid admission challenge");
        }
        return ("shacraft-admission-v1\n" + ticket + "\n" + serverId + "\n"
                + nickname + "\n" + nonce).getBytes(StandardCharsets.UTF_8);
    }

    public static String sign(String encodedPrivateKey, String ticket, String serverId,
                              String nickname, String nonce) throws Exception {
        if (encodedPrivateKey == null || encodedPrivateKey.length() > 256) {
            throw new IllegalArgumentException("Missing admission key");
        }
        byte[] encoded = Base64.getDecoder().decode(encodedPrivateKey);
        try {
            PrivateKey key = KeyFactory.getInstance("Ed25519")
                    .generatePrivate(new PKCS8EncodedKeySpec(encoded));
            Signature signer = Signature.getInstance("Ed25519");
            signer.initSign(key);
            signer.update(transcript(ticket, serverId, nickname, nonce));
            return Base64.getEncoder().encodeToString(signer.sign());
        } finally {
            java.util.Arrays.fill(encoded, (byte) 0);
        }
    }

    public static boolean validSignature(String value) {
        if (value == null || value.length() != 88) return false;
        try {
            byte[] decoded = Base64.getDecoder().decode(value);
            return decoded.length == 64 && Base64.getEncoder().encodeToString(decoded).equals(value);
        } catch (IllegalArgumentException invalid) {
            return false;
        }
    }

    public static boolean allowedTarget(String host, int port, boolean allowLoopback) {
        if (host == null) return false;
        if (allowLoopback && (host.equals("127.0.0.1") || host.equals("::1") || host.equals("[::1]") || host.equals("0:0:0:0:0:0:0:1"))) {
            return port > 0 && port <= 65535;
        }
        return port == 25568 && (host.equalsIgnoreCase("shacraft.ru") || host.equals("135.106.154.86"));
    }
}
