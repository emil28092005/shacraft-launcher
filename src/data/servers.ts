import type { Server } from '../types/launcher'

// Presentation metadata only. Rust reads install versions and managed files
// from the signed manifest; this list cannot control downloads or launch args.
export const servers: readonly [Server, ...Server[]] = [
  {
    id: 'aoc',
    kicker: 'Основная сборка',
    name: 'Aeronautics',
    subtitle: 'Строй корабли. Поднимай города в небо.',
    version: '1.21.1 · NeoForge 21.1.248',
    loader: 'NeoForge 21.1.248',
    profileId: 'aeronautics',
  },
  {
    id: 'minigames', kicker: 'Лобби и арены', name: 'Minigames',
    subtitle: 'Небесные острова. Сражения на аренах SMASH.',
    version: '26.2 · Fabric 0.19.5', loader: 'Fabric 0.19.5', profileId: 'minigames',
  },
]
