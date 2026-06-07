#!/usr/bin/env bash
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "Usage: $0 <new_version>"
  echo "Example: $0 0.4.0"
  exit 1
fi

NEW_VERSION="$1"

if [[ ! "$NEW_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: version must follow semver format (e.g. 0.4.0)"
  exit 1
fi

cd "$(git rev-parse --show-toplevel)"

CURRENT_VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml)

if [ -z "$CURRENT_VERSION" ]; then
  echo "Error: could not read version from Cargo.toml"
  exit 1
fi

echo "Current version: $CURRENT_VERSION"
echo "New version:     $NEW_VERSION"

if [ "$CURRENT_VERSION" = "$NEW_VERSION" ]; then
  echo "Nothing to do — version is already $NEW_VERSION"
  exit 0
fi

sed -i "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml

echo "Cargo.toml updated: version = \"$NEW_VERSION\""

git add Cargo.toml
git commit -m "chore: v$CURRENT_VERSION -> v$NEW_VERSION"

TAG="v$NEW_VERSION"
git tag -a "$TAG" -m "Release $TAG"

echo ""
echo "Done! To push:"
echo "  git push origin main && git push origin $TAG"
