# План ShaCraft Launcher

## Сделано: рефакторинг 2026-09-09

- [x] React components/hooks/typed IPC/state reducers вместо единого main.tsx.
- [x] Строгий TypeScript, тесты async lifecycle и последовательных сохранений.
- [x] Rust commands по ответственности; signed-only синхронизация профиля.
- [x] Общие atomic files, process-local operation guards, portable path checks.
- [x] Проверка подписи, размера envelope и profile ID; bounded provider redirects.
- [x] Push/PR проверки и ручная матрица сборки с артефактами.

## Следующие задачи

- [x] Сохранены изменения 0.1.1 из GitHub: обязательный ShaCraft-аккаунт,
  подтверждённый ник, реальный онлайн и исправления Windows-install pipeline.
- [ ] Microsoft (отдельное будущее решение): собственный client ID + API
  approval и живой OAuth-тест. Текущий запуск использует ShaCraft identity.
- [ ] Cold install / repair / update / game exit на чистых Windows/Linux/macOS.
  Unit tests и web preview не заменяют эти прогоны.
- [ ] Подписанные installer-релизы и подписанное автообновление лаунчера.
- [ ] Реальная отмена загрузок, журнал с редактированием токенов и retry UX.
- [ ] Выбор каталога профиля и безопасный reset только managed-файлов.
- [ ] Keychain-хранилище сессии ShaCraft; OS-lock и lease игры уже реализованы.
- [ ] Динамический каталог и новости; реальный Aeronautics онлайн уже
  загружается через фиксированный display-only API. Не имитировать данные.

## Исправления по handoff (исходники, до выкладки)

- [x] Один signed snapshot и общий межпроцессный lock на Play/Repair.
- [x] Inventory, journal/recovery, retirement старых неизменённых managed-файлов.
- [x] Явный перенос выбранных legacy-модов в резервную копию.
- [x] NeoForge clean rebuild + provenance receipt; непустая порча обнаруживается.
- [x] Отдельный первый вход с серверным grant и одноразовым proof.
- [x] RAM retry сохраняет намерение; неверный JAVA_HOME не скрывает подходящий PATH.
- [x] Версии в интерфейсе берутся из проверенного manifest.
- [ ] Совместная выкладка backend/Game Bridge/подписанного payload и нового лаунчера.
- [ ] Изолированная игровая проверка LoginSystem + hold + proof, затем beta по ОС.

Сопутствующий серверный код содержит read-only status, nonce proof с legacy
migration и durable grant/revoke outbox. Оплата и доставка whitelist — разные
состояния. Реферальные правила остаются отдельной задачей серверного PLAN;
это исправление не меняет условия покупки. Продакшен не изменён.
