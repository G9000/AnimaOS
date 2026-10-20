#!/usr/bin/env bash
# Sign anima release archives so the updater accepts them.
#
#   sign-assets.sh <version> <secret-key-file> <archive>...
#
# Each archive must be named anima-<target>.tar.gz or anima-<target>.zip, with <target>
# a Rust target triple. Writes <archive>.minisig next to it: a prehashed minisign
# signature over the archive whose trusted comment is
#
#   anima <version> <target> file:<archive name>
#
# <version> is the release tag without its leading "v" (0.2.0 for v0.2.0). The updater
# rebuilds this comment from the release it found and refuses any other
# (release_trusted_comment in hosts/anima/src/update/verify.rs).
#
# The secret key's password is read from MINISIGN_PASSWORD (unset or empty for an
# unencrypted key) and handed to minisign on stdin. MINISIGN names the minisign
# binary (default: minisign).
set -euo pipefail

fail() {
  echo "sign-assets: $*" >&2
  exit 1
}

[ "$#" -ge 3 ] || fail "usage: sign-assets.sh <version> <secret-key-file> <archive>..."
version=$1
key=$2
shift 2

# Semantic versioning, as the updater's semver parser reads the tag.
number='(0|[1-9][0-9]*)'
identifiers='[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*'
[[ $version =~ ^$number\.$number\.$number(-$identifiers)?(\+$identifiers)?$ ]] ||
  fail "version must be semver without a leading v, got '$version'"
[ -f "$key" ] || fail "no secret key file at '$key'"

# Check every name before signing anything.
targets=()
for archive in "$@"; do
  [ -f "$archive" ] || fail "no file at '$archive'"
  asset=$(basename -- "$archive")
  case $asset in
    anima-*.tar.gz) target=${asset%.tar.gz} ;;
    anima-*.zip) target=${asset%.zip} ;;
    *) fail "not a release archive (anima-<target>.tar.gz or .zip): '$asset'" ;;
  esac
  target=${target#anima-}
  [[ $target =~ ^[0-9A-Za-z_]+(-[0-9A-Za-z_]+)+$ ]] || fail "no target triple in '$asset'"
  targets+=("$target")
done

i=0
for archive in "$@"; do
  asset=$(basename -- "$archive")
  comment="anima $version ${targets[$i]} file:$asset"
  # A here-string, not a pipe: minisign skips reading it for an unencrypted key.
  "${MINISIGN:-minisign}" -S -H -s "$key" -m "$archive" -x "$archive.minisig" -t "$comment" \
    <<<"${MINISIGN_PASSWORD-}"
  echo "signed $asset: $comment"
  i=$((i + 1))
done
