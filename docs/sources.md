# Матрица источников

Это перечень **реализованного кода**, а не подтверждение доступности API на 11 сентября 2026 года. Живые запросы не выполнялись. Условия источников, авторизация и лимиты требуют проверки перед выпуском; free tier не означает отсутствие ключа, аккаунта или коммерческих ограничений.

## Встроенные адаптеры

| ID | Вход | Результат | Ограничение текущего кода |
|---|---|---|---|
| cloudflare | domain | A → IP | Один DNS RR type, JSON DoH |
| google | domain | A → IP | Один DNS RR type |
| crtsh | domain | CT → domain | До 255 дочерних узлов; без pagination |
| certspotter | domain | CT → domain | До 255 дочерних узлов; без pagination |
| internetdb | ip | hostnames → domain | Порты, CPE и CVE пока не извлекаются |
| urlscan | domain | passive search → URL | Только поиск; scan submission отсутствует |
| wayback | domain | CDX → URL | До 100 результатов; без pagination |
| rdap | domain | JSON в атрибуты | Только endpoint Verisign .com; нет bootstrap для других TLD |
| github | username | Публичный профиль | Без authenticated tier; до 48 KiB retained JSON |
| hackernews | username | Профиль | Только публичные поля |
| keybase | username | Публичный lookup | Живая доступность не проверена |

Дополнительный поставляемый манифест `google-aaaa.json` добавляет AAAA без пересборки. Все встроенные трансформации относятся к группе `data`. При исчерпании quota провайдера приложение не обходит ограничение, а показывает ошибку/повторяет временные сбои с задержкой.

## Не реализованы

| Запрошенные источники/возможности | Текущий статус |
|---|---|
| HackerTarget, universal RDAP, WHOIS | Нужны дополнительные адаптеры и протокольные/endpoint fixtures |
| ip-api, ipinfo, GreyNoise, Robtex, AbuseIPDB | Нет адаптеров; не считать автоматически доступными без ключей |
| HTTP headers | Нет активного fetch произвольного URL; для него нужны отдельные host/network scope правила |
| Reddit, Mastodon | Нет адаптеров и авторизации/выбора instance |
| Telegram public channels | Нет парсера публичных страниц или MTProto integration |
| ThreatFox, URLhaus, PhishTank, OpenPhish, OTX | Нет адаптеров; актуальные условия API не проверены |
| HIBP email breach search | Нет адаптера; необходим subscription key |
| BreachDirectory | Нет адаптера; актуальные условия доступа не проверены |
| Keyed/API-authenticated variants | Vault есть, network authentication adapter ещё не подключён |

HIBP отдельно документирует бесплатный Pwned Passwords API; это другой сервис, не бесплатный эквивалент поиска утечек по email. [Официальная API-документация HIBP](https://haveibeenpwned.com/API/v3).

## Условия добавления адаптера

1. Зафиксировать официальный endpoint, действующую авторизацию и rate limits.
2. Добавить записанные fixtures с пустым ответом, ошибкой, пагинацией и лимитом размера.
3. Указать семантику relation и claim provenance; не повышать confidence по одному совпадению строк.
4. Проверить маршруты proxy и offline, отмену, TTL и отсутствие credential/query в логах.
5. Для keyed adapter хранить секрет только в vault и использовать его только в запросе соответствующему провайдеру; очищать transient copies.
6. Выполнить live smoke test на общедоступном example/test target, записать дату и не выдавать эту проверку за гарантию будущей доступности.
