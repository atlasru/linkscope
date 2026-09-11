# Проверка GitHub Actions — 11 сентября 2026

Репозиторий: [atlasru/linkscope](https://github.com/atlasru/linkscope).

Исходники, прошедшие проверки:
`765d3cc49cadd87971f8388fb469a05b6ac146c0`.

- [Check source](https://github.com/atlasru/linkscope/actions/runs/34642664816): SUCCESS; Cargo check на Windows, Linux и macOS.
- [Desktop build](https://github.com/atlasru/linkscope/actions/runs/34642664740): SUCCESS на трёх ОС; установщики сохранены в Artifacts.
- 13 Rust-тестов и 5 JavaScript-тестов пройдены. Windows-тест восстановления журнала также прошёл.

В ходе CI исправлены две реальные проблемы:

1. Future планировщика трансформаций не удовлетворял Send из-за lifetime closure. Futures теперь материализуются как `BoxFuture<'static, ...>`; добавлен тест `tokio::spawn`.
2. Windows запрещал усечение журнала через append-only file handle. Recovery использует отдельный writable handle при удержании exclusive lock каталога; исходный тест восстановления сохранён и проходит.

## Границы проверки

Успех сборки не подтверждает запуск GUI, работу WebGL-шейдеров, показатели T480, общий RSS или отсутствие DNS/сетевых утечек. Локальная среда задания всё ещё не имеет Rust/Cargo и браузерного executable; нативные проверки выполнены на runners GitHub Actions.

Сборки не подписаны. Источники OSINT не проверялись живыми запросами. STIX/MISP serializers не проверялись внешним validator/import. Остальные незавершённые требования перечислены в acceptance.md.

Каждая build job сохраняет Cargo.lock и package-lock.json в отдельный артефакт dependency-locks. Они ещё не зафиксированы в Git; последующее разрешение semver-зависимостей может дать другие версии.
