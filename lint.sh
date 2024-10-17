#!/usr/bin/env bash

set -ex
cargo clippy -- -D warnings
cargo test
