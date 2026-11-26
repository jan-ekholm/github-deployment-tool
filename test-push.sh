#!/usr/bin/env bash

# Usage: WEBHOOK_SECRET=... ./test-push.sh repo branch [url]
set -euo pipefail

repo="${1}"
branch="${2}"
url="${3:-http://localhost:8080/}"

body="{\"ref\":\"refs/heads/$branch\",\"after\":\"abc123\",\"repository\":{\"full_name\":\"$repo\"}}"
sig=$(printf '%s' "$body" | openssl dgst -sha256 -hmac "$WEBHOOK_SECRET" | awk '{print $NF}')

curl -i -X POST $url \
  -H "Content-Type: application/json" \
  -H "X-GitHub-Event: push" \
  -H "X-Hub-Signature-256: sha256=$sig" \
  --data-binary "$body"
