package ru.shacraft.admission;

import static org.junit.jupiter.api.Assertions.*;
import java.nio.charset.StandardCharsets;
import java.security.KeyPairGenerator;
import java.security.Signature;
import java.util.Base64;
import org.junit.jupiter.api.Test;

class AdmissionProofTest {
    private static final String TICKET = "A".repeat(43), NONCE = "B".repeat(43);
    @Test void signsExactServerBoundTranscript() throws Exception {
        var pair=KeyPairGenerator.getInstance("Ed25519").generateKeyPair();
        String signed=AdmissionProof.sign(Base64.getEncoder().encodeToString(pair.getPrivate().getEncoded()), TICKET,"minigames","Pilot_1",NONCE);
        var verifier=Signature.getInstance("Ed25519"); verifier.initVerify(pair.getPublic());
        verifier.update(("shacraft-admission-v1\n"+TICKET+"\nminigames\nPilot_1\n"+NONCE).getBytes(StandardCharsets.UTF_8));
        assertTrue(verifier.verify(Base64.getDecoder().decode(signed)));
        verifier.update(AdmissionProof.transcript(TICKET,"minigames","Other",NONCE));
        assertFalse(verifier.verify(Base64.getDecoder().decode(signed)));
    }
    @Test void rejectsCrossServerAndMalformedFields() {
        for (String[] v:new String[][]{{TICKET,"aoc","Pilot",NONCE},{TICKET,"minigames","Bad\nName",NONCE},{"bad","minigames","Pilot",NONCE},{TICKET,"minigames","Pilot","bad"}})
            assertThrows(IllegalArgumentException.class,()->AdmissionProof.transcript(v[0],v[1],v[2],v[3]));
    }
    @Test void trustsOnlyMinigamesSocketAndExplicitLocalTests() {
        assertTrue(AdmissionProof.allowedTarget("shacraft.ru",25568,false));
        assertTrue(AdmissionProof.allowedTarget("135.106.219.182",25568,false));
        assertTrue(AdmissionProof.allowedTarget("135.106.154.86",25568,false));
        assertFalse(AdmissionProof.allowedTarget("135.106.219.183",25568,false));
        assertFalse(AdmissionProof.allowedTarget("135.106.219.182",25567,false));
        assertFalse(AdmissionProof.allowedTarget("135.106.154.86",25567,false));
        assertFalse(AdmissionProof.allowedTarget("127.0.0.1",25568,false));
        assertTrue(AdmissionProof.allowedTarget("127.0.0.1",25570,true));
        assertFalse(AdmissionProof.allowedTarget("attacker.invalid",25568,true));
    }
}
