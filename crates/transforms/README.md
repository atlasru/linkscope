# Public-data transformations

`Service::manifests()` declares compatible entity types and the remote destination.
`Service::run()` returns a proposed generic graph patch. The caller previews and
applies it explicitly. Only the selected entity label is sent; no project graph
or unrelated properties leave the application. Running a job is the network action.

- DNS sends A and AAAA queries to Google Public DNS. Only valid matching address
  families become attributed `resolves-to` observations.
- RDAP starts at rdap.org and may follow up to three public HTTPS registry
  redirects. The complete registration response is a separate record entity.
- Certificate transparency queries crt.sh for subdomain certificate records.
  Up to 100 distinct service record IDs become certificate entities; matching
  names never cause existing entities to merge or imply common ownership.

Each request preserves service URI, retrieval time, and original source records.
Cached raw responses preserve their original retrieval time and are converted
into fresh observations when replayed; the service does not inspect graph state.
Caching is process-local, expires after five minutes, and holds at most 64
responses / 16 MiB. No background refresh or telemetry runs.

At most two jobs are active and 32 jobs are admitted. Requests start at least
500 ms apart. The entire job, including queue/rate waits, has a 20-second deadline.
Cancellation drops waits and HTTP streams. Responses are streamed with a 2 MiB
cap. HTTP errors fail explicitly; automatic retries are intentionally omitted.
IPv4/IPv6 RDAP is supported for entities of kind `ip`; international domains must
be supplied as ASCII punycode. Proxy configuration is not exposed in this first
implementation, and environment proxy auto-discovery is disabled.

Regular tests use a deterministic in-process transport and never use the network:

```powershell
cargo test -p linkscope-transforms
```

An optional probe performs real requests for the public example.com domain:

```powershell
cargo run -p linkscope-transforms --example live_smoke
```

Passing mock tests does not establish availability or correctness of live public
services. crt.sh may exceed the deadline or body cap on broad queries; the user
receives a clear error instead of partial results being silently accepted.

Primary service contracts: [Google DNS JSON API](https://developers.google.com/speed/public-dns/docs/doh/json),
[RDAP](https://www.icann.org/rdap), [crt.sh](https://crt.sh/).
