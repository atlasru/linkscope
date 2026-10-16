# Transformation validation — 16 October 2026

Windows x64, GNU Rust toolchain, debug build. All checks were local; no repository
hosting state was changed.

`cargo test -p linkscope-transforms --offline -j 2`: **16 passed, 0 failed,
0 ignored**. Test execution took 1.07 seconds. The first dependency build took
3 minutes. Tests use an injected in-process transport and send no network traffic.
They cover parser output and provenance, applying/undoing a proposed graph patch,
cache replay/fresh observation IDs/original retrieval time/expiry/entry cap,
invalid input and service response handling, bounded concurrency/admission,
cancellation during HTTP and rate/concurrency waits, whole-job deadlines, request
start spacing, and allowed/rejected/looping registry redirects.

The opt-in `live_smoke` executable then made actual requests only for the public
`example.com` fixture. It exited successfully:

| Service | Output entities | Output relationships | Elapsed |
| --- | ---: | ---: | ---: |
| Google DNS A + AAAA | 4 | 4 | 1.2678815 s |
| RDAP bootstrap / registry | 1 registration record | 1 | 2.2827379 s |
| crt.sh certificate transparency | 81 certificate records | 81 | 10.5038113 s |

DNS returned no warnings. RDAP and certificate outputs each included their
documented caution about ownership inference. Results were generated as proposed
patches; the smoke program did not mutate or transmit any investigation project.
Counts and latency reflect these particular public responses, not a throughput
benchmark or a service availability guarantee.

The installed MSVC Visual Studio instance lacks its C++ tools/SDK. The working
GNU build environment puts `C:\msys64\mingw64\bin` before the Rust GNU toolchain's
`bin` directory and sets `CC` to that directory's `gcc.exe`; reversing those PATH
entries caused silent C compiler backend failures from conflicting runtime DLLs.
The rustup proxy also reported an access-denied cleanup error, so validation used
the installed GNU toolchain's `cargo.exe` directly. Source requires no special
runtime selection on a normally configured compiler installation.
