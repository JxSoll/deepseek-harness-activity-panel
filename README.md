# DSH — вспомогательное окно агента

Плагин для [DeepSeek Harness](https://github.com/deepseek-ai): небольшое
полупрозрачное окно поверх всех окон, которое показывает, что агент делает
прямо сейчас.

![Окно](https://raw.githubusercontent.com/JxSoll/deepseek-harness-activity-panel/main/docs/preview.png)

## Что показывает

- текущий ответ с оформлением как в чате: заголовки, таблицы, списки,
  блоки кода и инлайн-код плашками;
- строку «Глубокое размышление… N с», пока идёт рассуждение;
- таймер текущего хода;
- прошлый ответ, пока новый ещё не начался.

## Поведение

- правый нижний угол, растёт вверх по мере текста;
- не поднимается выше окна DSH и не заходит на панель задач;
- клики проходят сквозь окно;
- через 30 секунд после завершения работы плавно уезжает вниз;
- закрыть принудительно — `Ctrl+Alt+X`.

## Установка

### Вариант 1 — готовый exe (ничего собирать не нужно)

1. Скачайте `ActivityWindow.exe` из
   [последнего релиза](https://github.com/JxSoll/deepseek-harness-activity-panel/releases/latest).
2. Скачайте папку `plugin/` из репозитория.
3. Положите exe в `plugin/viewer/ActivityWindow.exe`.
4. В DSH: **Плагины → Add plugin** → укажите папку `plugin`.

Требования: Windows 10/11 и DSH Desktop. **Rust и .NET не нужны** — exe
собран статически, зависимостей нет.

### Вариант 2 — собрать окно самому

Нужны Rust и MinGW-w64 (toolchain `x86_64-pc-windows-gnu`):

```powershell
cargo build --release --manifest-path activity-rs/Cargo.toml
copy target\release\activity-window.exe plugin\viewer\ActivityWindow.exe
```

Статическая сборка без внешних DLL:

```powershell
$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --release --manifest-path activity-rs/Cargo.toml
```

## Структура

```
plugin/       — плагин DSH (bundle): манифест, патч, index.js, локали, иконка
activity-rs/  — исходник окна: Rust + eframe/egui
```

## Настройка

В `plugin/cordis.patch.yml` профиля можно отключить автозапуск окна:

```yaml
- id: dsh-activity
  name: '@local/dsh-activity-panel'
  config:
    showViewer: false
```

Состояние пишется в `%USERPROFILE%\.dsh\activity\state.json`.

## Лицензия

MIT
