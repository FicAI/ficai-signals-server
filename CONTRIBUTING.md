# Running tests

Besides whatever gets tested by `cargo test`, there are black-box integration tests. Their source is in [`test.sh`](/test.sh).

Tests require the following programs to be installed and accessible on `$PATH`:
- [shunit2](https://github.com/kward/shunit2/) — the test running engine, see the last line of `test.sh`
- `jq`
- `curl` version 7.76.0 or greater (for `--fail-with-body`)

The tests expect all variables needed to run the server to be available in either the environment or in the file `test.env` (ignored by git), which you can make for yourself by copying and modifying `test.env.template`. Take special care to match the IP address in `FICAI_LISTEN` and the value of `FICAI_DOMAIN`, otherwise `curl` invocations won't work. Also, since the server sets the authentication cookie as "secure", it seems that `curl` wants the target address to either be HTTPS or localhost; see [curl 7.79.0 release notes](https://daniel.haxx.se/blog/2021/09/15/curl-7-79-0-secure-local-cookies/).
