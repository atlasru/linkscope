# Приёмка: блокирующие проверки до релиза

Исходное ТЗ не считается выполненным этой поставкой. Ниже воспроизводимый план; значения не подставлены вместо реальных измерений.

| Требование | Статус | Доказательство, необходимое для приёмки |
|---|---|---|
| Rust/Tokio + Tauri | Исходники есть, не скомпилированы | Cargo tests и native build на трёх ОС |
| 100k nodes / 500k edges | Есть генератор UI-fixture | Полный путь import → Rust graph → persistence → render, RSS всех процессов |
| 50k / 200k: ≥40 FPS, ≤400 MB | Не измерено | Release на T480, Intel UHD 620, 1920×1080, 60 s после прогрева |
| UI при 10 трансформациях | Scheduler/queue реализованы, не испытаны | Фиктивные независимые источники, задержки/429/503, UI latency p95 |
| Нет system DNS/direct leaks | Политики в исходниках, packet capture отсутствует | DNS canary, proxy drop, redirect test, pcap всех интерфейсов |
| Embedded Tor / proxy chain | Не реализовано | Arti integration, bootstrap capture, isolation, ephemeral policy |
| Все перечисленные источники | Частично: 11 встроенных | Контрактные fixtures + live tests, актуальная матрица авторизации |
| Автоматические уровни intelligence | Только поле/визуализация | Claim corroboration / resolution / threat rules |
| GPU layout, incremental | Приближённый shader в исходниках | GL errors=0, finite positions, корректность freeze, benchmark |
| Louvain / hulls | JS-алгоритм и тест есть | Worker integration + stress/RSS/quality tests |
| Карта | Только пустая offline coordinate grid | Geo ingestion, выбранная геоподложка/лицензия, linked selection |
| Полный memory ceiling | Только оценка/admission графа | Process-tree sampler, bounded global admission, overload tests |
| Cold graph paging | Только payload eviction | Выгрузка топологии и LRU с политикой возраста |
| Age vault | Код и непроверенные Rust-тесты | Roundtrip, tamper, key rotation, memory clearing, no-disk plaintext |
| Ephemeral | Ветка core RAM реализована | File tracing Tauri/WebView/OS; policy для exports и swap |
| STIX 2.1 / MISP | Serializers есть | Официальный STIX validator / импорт в выбранную версию MISP |
| Установщики ≤50 MB | Не созданы | Release artifact sizes, зависимость WebView, подпись |
| Двойной клик / запуск ≤2 s | Не проверено | Cold/warm start и чистая offline VM на каждой ОС |

## Функциональный gate

```sh
cargo fmt --all
cargo test -p linkscope-core
cargo check -p linkscope
node --test tests/model.test.mjs
```

После разрешения зависимостей сохранить lockfiles. Проверить обработку ошибок без утечки секретов. Проверить OS-level lock каталога исследования двумя одновременно запускаемыми процессами.

## Проверка интерфейса в браузере

Это дополнительная инженерная проверка, а не замена native WebView benchmark:

```sh
npm install --no-save playwright
npx playwright install chromium
python3 -m http.server 8765 --bind 127.0.0.1 --directory ui
# во втором терминале
node tests/browser.mjs
```

`tests/browser.mjs` проверяет выбор демо, поиск, GPU layout, finite positions, Louvain worker, timeline и 50k dataset; сохраняет screenshots/frame times/heap metadata. Для измерений включить `HEADED=1` и проверить, что renderer аппаратный. SwiftShader/llvmpipe не подходят для приёмки T480. JS heap не равен общей памяти приложения.

## Native performance protocol

1. ThinkPad T480, i5-8250U, 16 GiB, Intel UHD 620; записать ОС, драйвер, release SHA и режим питания.
2. Импортировать в **Rust** 50 000 уникальных сущностей и 200 000 рёбер пакетами; синтетический UI-mode для этой проверки недостаточен. Backend bulk-import/benchmark harness ещё требуется реализовать.
3. Измерять RSS/PSS backend и всех WebView-процессов, GPU allocations отдельно; считать peak и steady state. Задать правила трактовки MB/MiB заранее.
4. После 10 s прогрева записать 60 s frame times при pan/zoom, поиске и потоке результатов. Указать p50/p95/p99; не считать idle-on-demand FPS провалом или успехом.
5. Повторить с layout/cluster, пустым/тёплым HTTP-кэшем и длинными атрибутами.
6. При превышении бюджета проверить отказ новых данных без падения, целостность SQLite и восстановление после kill между log fsync и SQL commit.

## Сетевая приёмка

В чистой VM без постороннего фонового трафика записать pcap и процессную атрибуцию:

- Cold start Offline, 60 s ожидания, поиск, layout и export: нет исходящих соединений приложения.
- SOCKS5H/CONNECT к прокси с фиксированным IP: только прокси destination; нет UDP/TCP 53 и локального разрешения target host.
- Недоступный прокси, 302 на другой хост, поддельный сертификат: ошибки без fallback.
- Direct DoH: отдельно ожидаются bootstrap IP выбранного resolver и HTTPS-источники; этот режим явно не анонимный.
- Cancel и смена proxy policy: нет оставшихся запросов старого задания; уже записанные данные остаются.
- Arti, когда появится: отдельно разрешённый bootstrap/guard traffic и проверка отсутствия прямых запросов к источникам.

«Полная анонимность» не выводится из одного Wireshark-capture: это проверка конкретных каналов утечки при заданных условиях.
