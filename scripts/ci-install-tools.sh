#!/bin/bash
# Install the system libraries this repo's build links against and the
# external tools its tests shell out to. The canonical ci.yml runs this in
# every repo before `cargo build`; a repo that needs neither keeps this
# script as an explicit no-op. Keep it strict: anything that fails to
# install must fail the build here, not surface later as a confusing
# build or test failure.
set -euo pipefail

# rspass shells out to gpg for all encryption, exactly like pass(1).
# ubuntu-latest ships gnupg, but install explicitly so a runner image change
# fails here with a clear message instead of inside the tests.
#
# Acquire::Retries because apt's default is 0: when the first mirror in
# /etc/apt/apt-mirrors.txt is unreachable there is no second attempt;
# Retries=3 lets apt fall through to archive.ubuntu.com.
sudo apt-get -o Acquire::Retries=3 update
sudo apt-get -o Acquire::Retries=3 install -y gnupg
