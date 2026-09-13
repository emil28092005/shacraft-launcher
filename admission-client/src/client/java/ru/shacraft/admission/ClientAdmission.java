package ru.shacraft.admission;

import java.net.InetSocketAddress;
import java.util.concurrent.atomic.AtomicBoolean;
import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.networking.v1.ClientConfigurationNetworking;
import net.fabricmc.fabric.api.client.networking.v1.ClientConfigurationConnectionEvents;
import net.fabricmc.fabric.impl.networking.RegistrationPayload;
import net.minecraft.network.protocol.common.ServerboundCustomPayloadPacket;
import java.util.List;
import net.fabricmc.fabric.api.networking.v1.PayloadTypeRegistry;
import net.minecraft.network.chat.Component;

/** The account session remains in the native launcher, never in Minecraft. */
public final class ClientAdmission implements ClientModInitializer {
    private static final AtomicBoolean USED = new AtomicBoolean();

    @Override public void onInitializeClient() {
        PayloadTypeRegistry.clientboundConfiguration().register(AdmissionPayloads.Challenge.TYPE, AdmissionPayloads.Challenge.CODEC);
        PayloadTypeRegistry.serverboundConfiguration().register(AdmissionPayloads.Proof.TYPE, AdmissionPayloads.Proof.CODEC);
        ClientConfigurationNetworking.registerGlobalReceiver(AdmissionPayloads.Challenge.TYPE, ClientAdmission::challenge);
        // Paper waits for vanilla channel advertisement, while Fabric normally waits
        // for the server's registration first. Bootstrap our one fixed receiver.
        // INIT runs in the listener constructor; schedule() queues until after vanilla
        // has switched the outbound protocol from LOGIN to CONFIGURATION.
        ClientConfigurationConnectionEvents.INIT.register((listener, client) -> client.schedule(() ->
            listener.send(new ServerboundCustomPayloadPacket(new RegistrationPayload(
                RegistrationPayload.REGISTER, List.of(AdmissionPayloads.Challenge.TYPE.id()))))));
    }

    private static void challenge(AdmissionPayloads.Challenge challenge, ClientConfigurationNetworking.Context context) {
        var connection = context.packetContext().orElseThrow(net.fabricmc.fabric.api.networking.v1.context.PacketContext.CONNECTION);
        boolean loopback = "1".equals(System.getenv("SHACRAFT_ADMISSION_ALLOW_LOOPBACK"));
        if (!(connection.getRemoteAddress() instanceof InetSocketAddress remote)
                || remote.getAddress() == null
                || !AdmissionProof.allowedTarget(remote.getAddress().getHostAddress(), remote.getPort(), loopback)
                || !AdmissionProof.SERVER_ID.equals(challenge.serverId())
                || !context.client().getUser().getName().equals(challenge.nickname())
                || !AdmissionProof.validOpaque(challenge.nonce())) {
            deny(context); return;
        }
        String ticket = System.getenv("SHACRAFT_ADMISSION_TICKET");
        String privateKey = System.getenv("SHACRAFT_ADMISSION_PRIVATE_KEY");
        if (!AdmissionProof.validOpaque(ticket) || !USED.compareAndSet(false, true)) {
            deny(context); return;
        }
        try {
            String signature = AdmissionProof.sign(privateKey, ticket, challenge.serverId(), challenge.nickname(), challenge.nonce());
            context.responseSender().sendPacket(new AdmissionPayloads.Proof(ticket, signature));
        } catch (Exception invalidKey) { deny(context); }
    }

    private static void deny(ClientConfigurationNetworking.Context context) {
        context.responseSender().disconnect(Component.literal(
            "Не удалось подтвердить вход ShaCraft. Закройте игру и запустите её заново через ShaCraft Launcher."));
    }
}
