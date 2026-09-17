# ShaCraft Minigames admission client

Client-only Fabric companion for Minecraft 26.2, Java 25, Fabric Loader 0.19.5
and Fabric API 0.160.0+26.2. This is account admission, not an anti-cheat or a
proof that the original launcher binary is running.

Build with Java 25: `./gradlew test build --no-daemon`.
Output: `build/libs/shacraft-admission-client-0.1.0.jar`.
Publish that jar and the pinned Fabric API jar in the signed Minigames profile.

The native launcher supplies `SHACRAFT_ADMISSION_TICKET` and
`SHACRAFT_ADMISSION_PRIVATE_KEY` only to the final Java child environment.
The client accepts a CONFIGURATION payload on `shacraft_admission:challenge`
with three Minecraft UTF strings: server ID (16), nickname (16), nonce (43).
It verifies server `minigames`, the exact current game nickname and actual
socket `135.106.219.182:25568` (the previous IP remains accepted during migration), then signs once with the ephemeral Ed25519 key.
The response on `shacraft_admission:proof` contains ticket (43) and standard
Base64 signature (88). The transcript has no final newline:

```
shacraft-admission-v1
{ticket_id}
minigames
{mc_username}
{nonce}
```

The private key and website session never go onto the Minecraft wire. Errors
are redacted. A fresh game launch is needed for another connection after a
proof has been sent. `SHACRAFT_ADMISSION_ALLOW_LOOPBACK=1` additionally permits
literal loopback sockets for isolated tests; normal releases do not set it.
Paper must fail closed before world entry and reject unauthenticated duplicate
UUIDs before the vanilla duplicate-player eviction. The backend checks the
current shared aoc account access and atomically redeems the server-bound ticket.

Three unit tests cover exact signature binding, invalid/cross-server fields
and socket allowlisting. The unchanged production companion also passed actual Minecraft 26.2
configuration negotiation and entered a local Paper lobby with a synthetic
backend ticket; see [receipt](../docs/verification/minigames-fabric-2026-09-13.json).
The public server and other platforms still require their own rollout checks.

The client explicitly advertises its single challenge receiver with vanilla
`minecraft:register` at the start of configuration. Fabric normally waits for
the server's registration first, while Paper gates plugin sends on that client
advertisement. This bootstrap uses the pinned Fabric API's RegistrationPayload;
update it and rerun live negotiation checks when upgrading Fabric API. It is
queued after INIT so vanilla has switched outbound protocol to CONFIGURATION.
