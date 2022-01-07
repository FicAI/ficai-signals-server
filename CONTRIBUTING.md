# Running tests

Besides whatever gets tested by `cargo test`, there are black-box integration tests. Their source is in [`test.sh`](/test.sh). They are based on [shunit2](https://github.com/kward/shunit2/) and require it to be accessible on `$PATH` — see the last line of `test.sh`.
