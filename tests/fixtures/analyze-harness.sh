#!/bin/sh
set -eu

if [ -f "$AUTOSHADE_TEST_HARNESS_FAIL" ]; then
  printf '%s\n' 'synthetic harness failure' >&2
  exit 9
fi

mkdir -p "$AUTOSHADE_TEST_DEVELOP"
cp "$AUTOSHADE_TEST_RECIPE" "$AUTOSHADE_TEST_DEVELOP/recipe.json"
if [ -f "$AUTOSHADE_TEST_HARNESS_FAIL_AFTER_COMMIT" ]; then
  printf '%s\n' 'synthetic harness failure after commit' >&2
  exit 10
fi
printf '{"develop":"%s","verdict":"accept","score":91,"session":"synthetic-http-session"}\n' \
  "$AUTOSHADE_TEST_DEVELOP" > "$AUTOSHADE_SESSION_SUMMARY"
