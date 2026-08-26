#!/usr/bin/env bash
# End-to-end smoke test against a running server.
# Usage: scripts/smoke.sh [base-url]
set -uo pipefail

BASE="${1:-http://127.0.0.1:23130}/api/v1"
PASS=0
FAIL=0

check() { # check <name> <value>
  if [[ -n "${2:-}" && "$2" != "null" && "$2" != "false" && "$2" != "0" ]]; then
    printf '  \033[32mok\033[0m   %-44s %s\n' "$1" "$2"
    PASS=$((PASS + 1))
  else
    printf '  \033[31mFAIL\033[0m %-44s %s\n' "$1" "${2:-<empty>}"
    FAIL=$((FAIL + 1))
  fi
}

j() { curl -sS -H 'Content-Type: application/json' "$@"; }

echo "▸ system"
check "ping"             "$(j "$BASE/ping" | jq -r .ok)"
check "schema types"     "$(j "$BASE/schema" | jq -r '.itemTypes | length')"
LIB=$(j "$BASE/libraries" | jq -r '.[0].id')
check "library id"       "$LIB"

echo "▸ collections"
COLL=$(j -X POST "$BASE/libraries/$LIB/collections" -d '{"name":"Smoke Test"}' | jq -r .key)
check "create"           "$COLL"
check "list"             "$(j "$BASE/libraries/$LIB/collections" | jq -r 'length')"

echo "▸ items"
CREATE=$(j -X POST "$BASE/libraries/$LIB/items" -d "$(cat <<JSON
[
  {"itemType":"journalArticle","title":"Attention Is All You Need",
   "abstractNote":"We propose the Transformer, a model based solely on attention mechanisms.",
   "date":"2017-06-12","DOI":"10.48550/arXiv.1706.03762",
   "creators":[{"creatorType":"author","firstName":"Ashish","lastName":"Vaswani"}],
   "tags":[{"tag":"transformer"},{"tag":"nlp"}],
   "collections":["$COLL"]},
  {"itemType":"journalArticle","title":"扩散模型在分子生成中的应用综述",
   "abstractNote":"本文综述了扩散模型用于三维分子构象生成的最新进展。",
   "date":"2023","creators":[{"creatorType":"author","name":"张伟","fieldMode":1}],
   "tags":[{"tag":"综述"},{"tag":"diffusion"}]},
  {"itemType":"nonsenseType","title":"should fail"}
]
JSON
)")
check "batch created"    "$(echo "$CREATE" | jq -r '.created | length')"
check "batch failed"     "$(echo "$CREATE" | jq -r '.failed | length')"
KEY=$(echo "$CREATE" | jq -r '.created[0].key')
VER=$(echo "$CREATE" | jq -r '.created[0].version')

check "read back"        "$(j "$BASE/libraries/$LIB/items/$KEY" | jq -r .title)"
check "list total"       "$(j "$BASE/libraries/$LIB/items?limit=10" | jq -r '.total')"
check "collection scope" "$(j "$BASE/libraries/$LIB/items?collection=$COLL" | jq -r '.total')"
check "patch"            "$(j -X PATCH "$BASE/libraries/$LIB/items/$KEY" \
                             -H "If-Unmodified-Since-Version: $VER" \
                             -d '{"fields":{"volume":"30"}}' | jq -r .volume)"
check "stale patch -> 412" "$(curl -sS -o /dev/null -w '%{http_code}' -X PATCH \
                             "$BASE/libraries/$LIB/items/$KEY" \
                             -H 'Content-Type: application/json' \
                             -H "If-Unmodified-Since-Version: $VER" \
                             -d '{"fields":{"volume":"31"}}')"
check "bad key -> 422"   "$(curl -sS -o /dev/null -w '%{http_code}' "$BASE/libraries/$LIB/items/not%20a%20key")"

echo "▸ search"
sleep 1.5   # let the embedding worker drain the queue
check "keyword"          "$(j "$BASE/libraries/$LIB/search?q=attention&mode=keyword" | jq -r '.hits|length')"
check "fuzzy (typo)"     "$(j "$BASE/libraries/$LIB/search?q=attension&mode=fuzzy" | jq -r '.hits|length')"
check "semantic"         "$(j "$BASE/libraries/$LIB/search?q=neural%20sequence%20model&mode=semantic" | jq -r '.hits|length')"
check "chinese"          "$(j "$BASE/libraries/$LIB/search?q=%E6%89%A9%E6%95%A3%E6%A8%A1%E5%9E%8B" | jq -r '.hits|length')"
check "tag operator"     "$(j "$BASE/libraries/$LIB/search?q=tag:nlp" | jq -r '.hits|length')"
check "snippet mark"     "$(j "$BASE/libraries/$LIB/search?q=transformer" | jq -r '.hits[0].snippet' | grep -c '<mark>')"
check "items?q= hydrate" "$(j "$BASE/libraries/$LIB/items?q=attention" | jq -r '.items[0].match.sources|length')"

echo "▸ tags & facets"
check "tags"             "$(j "$BASE/libraries/$LIB/tags" | jq -r 'length')"
check "facets"           "$(j "$BASE/libraries/$LIB/facets" | jq -r 'length')"

echo "▸ plugins"
check "plugin list"      "$(j "$BASE/plugins" | jq -r 'type')"
check "contributions"    "$(j "$BASE/plugins/contributions" | jq -r 'type')"

echo "▸ trash"
check "trash"            "$(j -X DELETE "$BASE/libraries/$LIB/items" -d "{\"keys\":[\"$KEY\"]}" | jq -r .trashed)"
check "trash view"       "$(j "$BASE/libraries/$LIB/items?trash=only" | jq -r .total)"
check "restore"          "$(j -X POST "$BASE/libraries/$LIB/items/restore" -d "{\"keys\":[\"$KEY\"]}" | jq -r .restored)"

echo "▸ stats"
check "stats items"      "$(j "$BASE/stats" | jq -r .items)"
check "search stats"     "$(j "$BASE/search/stats" | jq -r .documents)"
check "embedded vectors" "$(j "$BASE/search/stats" | jq -r .embedded)"

echo
if [[ $FAIL -eq 0 ]]; then
  printf '\033[32m%d checks passed\033[0m\n' "$PASS"
else
  printf '\033[31m%d passed, %d failed\033[0m\n' "$PASS" "$FAIL"
  exit 1
fi
