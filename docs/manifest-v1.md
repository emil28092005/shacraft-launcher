# ShaCraft manifest v1

Каждая игровая сборка описывается одним JSON-документом. Лаунчер принимает
документ только после проверки подписи релизного ключа и затем валидирует эту
схему до начала загрузки.

```json
{
  "schemaVersion": 1,
  "id": "aeronautics",
  "displayName": "All of Create Aeronautics",
  "minecraft": {
    "version": "1.21.1",
    "loader": { "kind": "neoforge", "version": "21.1.248" },
    "javaMajor": 21
  },
  "files": [{
    "path": "mods/example.jar",
    "url": "https://cdn.shacraft.ru/aeronautics/example.jar",
    "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "size": 42,
    "policy": "managed"
  }]
}
```

`managed` — файл контролирует лаунчер: при несовпадении SHA-256 он заменяется.
`seed` — файл создаётся только при первом запуске и затем сохраняет изменения
игрока. В manifest v1 допускаются только HTTPS-адреса на `shacraft.ru` и
`cdn.shacraft.ru`, а также относительные пути без `..`, обратных слешей и
пустых сегментов.
