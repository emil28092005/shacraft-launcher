# План ShaCraft Launcher

## Сделано: рефакторинг 2026-09-09

- [x] React components/hooks/typed IPC/state reducers вместо единого main.tsx.
- [x] Строгий TypeScript, тесты async lifecycle и последовательных сохранений.
- [x] Rust commands по ответственности; signed-only синхронизация профиля.
- [x] Общие atomic files, process-local operation guards, portable path checks.
- [x] Проверка подписи, размера envelope и profile ID; bounded provider redirects.
- [x] Push/PR проверки и ручная матрица сборки с артефактами.

## Следующие задачи

- [ ] Microsoft: собственный public-client ID + Minecraft API approval;
  затем живой device-code/login/refresh/logout тест. Сейчас ID — placeholder.
- [ ] Cold install / repair / update / game exit на чистых Windows/Linux/macOS.
  Unit tests и web preview не заменяют эти прогоны.
- [ ] Подписанные installer-релизы и подписанное автообновление лаунчера.
- [ ] Реальная отмена загрузок, журнал с редактированием токенов и retry UX.
- [ ] Выбор каталога профиля и безопасный reset только managed-файлов.
- [ ] Keychain-хранилище refresh token; cross-process exclusion при необходимости.
- [ ] Динамический каталог/онлайн серверов, новости и ссылки сообщества.
  Недоступные функции сейчас отключены, данные не имитируются.

## Связанные серверные риски

Серверный план находится в `/root/shacraft/PLAN.md`. Важные следующие шаги:
одноразовое подтверждение ника внутри игры (NoGravity не связывает игрока
с веб-запросом), enforcement реферальных правил и очередь повторов whitelist.
Не менять этот протокол незаметно в клиентском рефакторинге.
