package ru.shacraft.admission;

import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.network.codec.StreamCodec;
import net.minecraft.network.protocol.common.custom.CustomPacketPayload;
import net.minecraft.resources.Identifier;

public final class AdmissionPayloads {
    private AdmissionPayloads() {}

    public record Challenge(String serverId, String nickname, String nonce) implements CustomPacketPayload {
        public static final Type<Challenge> TYPE = new Type<>(Identifier.fromNamespaceAndPath("shacraft_admission", "challenge"));
        public static final StreamCodec<FriendlyByteBuf, Challenge> CODEC = StreamCodec.of(
                (buffer, value) -> { buffer.writeUtf(value.serverId, 16); buffer.writeUtf(value.nickname, 16); buffer.writeUtf(value.nonce, 43); },
                buffer -> new Challenge(buffer.readUtf(16), buffer.readUtf(16), buffer.readUtf(43)));
        @Override public Type<Challenge> type() { return TYPE; }
        @Override public String toString() { return "AdmissionChallenge[redacted]"; }
    }

    public record Proof(String ticket, String signature) implements CustomPacketPayload {
        public static final Type<Proof> TYPE = new Type<>(Identifier.fromNamespaceAndPath("shacraft_admission", "proof"));
        public static final StreamCodec<FriendlyByteBuf, Proof> CODEC = StreamCodec.of(
                (buffer, value) -> { buffer.writeUtf(value.ticket, 43); buffer.writeUtf(value.signature, 88); },
                buffer -> new Proof(buffer.readUtf(43), buffer.readUtf(88)));
        @Override public Type<Proof> type() { return TYPE; }
        @Override public String toString() { return "AdmissionProof[redacted]"; }
    }
}
