#!/usr/bin/env bash

set -ex
cargo clippy -- -Dwarnings -A clippy::await_holding_lock
cargo test
