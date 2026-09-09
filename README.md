# ShaCraft Launcher

Кроссплатформенный Tauri 2 лаунчер для [ShaCraft](https://shacraft.ru/):
React/TypeScript интерфейс, Rust — файлы, сеть и запуск процессов.

Реализованы подписанная синхронизация Aeronautics, проверка/восстановление
модов и конфигурации, Java discovery/provisioning, bootstrap Minecraft и
NeoForge, настройки памяти, обработка установки/запуска/выхода. Проверка
сборки теперь восстанавливает и игровые файлы; старые неизменённые managed-моды
убираются в резервную копию, неизвестные моды разбираются явно пользователем.

Вход выполняется через аккаунт ShaCraft — тот же, что на сайте. Игровой ник
берётся только из подтверждённой привязки Aeronautics, а не из редактируемых
локальных настроек. Пароли не сохраняются; сессию можно отозвать.
Для первого входа без привязки добавлено отдельное действие в настройках
аккаунта: установка → серверное разрешение → LoginSystem → одноразовая команда
подтверждения. Оно требует совместной выкладки backend, Game Bridge и
подписанного модпака; наличие исходников не означает публикацию этого сценария.
Microsoft OAuth-модуль сохранён отдельно, но не используется текущим
сценарием запуска; для его активации потребуются client ID и API approval.

## Разработка

```bash
npm ci
npm run dev
```

Это браузерный preview — он не устанавливает и не запускает игру.
Для приложения нужен Rust и системные зависимости Tauri:

```bash
npm run tauri:dev
```

## Проверка

```bash
npm test
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Build включает строгий TypeScript. GitHub Actions проверяет UI и Rust на
push/PR; workflow на main-push/ручном запуске собирает Windows x64, Linux x64, macOS Intel
и Apple Silicon и сохраняет артефакты. Подпись релиза/автообновления ещё впереди.
Используемые macOS runners соответствуют [списку GitHub](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

## Навигация

- [Архитектура](docs/launcher-architecture.md) — компоненты, данные, IPC.
- [Trust boundaries](docs/game-trust-boundary.md) — доверенные источники игры.
- [Manifest](docs/manifest-v1.md) — подписанный контракт модпака.
- [PLAN.md](PLAN.md) — ограничения и следующие шаги.
- [AGENTS.md](AGENTS.md) — инструкции для следующего разработчика/агента.

Не хранить в Git токены, ключи, пользовательские данные или пакеты игры.
