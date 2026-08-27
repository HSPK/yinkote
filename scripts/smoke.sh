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

echo "▸ collection appearance"
APPK=$(j -X POST "$BASE/libraries/$LIB/collections" \
         -d '{"name":"Smoke appearance","color":"violet","icon":"flask"}' | jq -r .key)
check "colour saved"     "$(j "$BASE/libraries/$LIB/collections" \
                            | jq -r --arg k "$APPK" '.[] | select(.key==$k) | .color')"
check "colour cleared"   "$(j -X PATCH "$BASE/libraries/$LIB/collections/$APPK" -d '{"color":null}' \
                            | jq -r 'if .color then "kept" else .icon end')"

echo "▸ zotero import"
ZDB=$(mktemp -d)/zotero.sqlite
python3 - "$ZDB" <<'PYEOF'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.executescript('''
CREATE TABLE itemTypes (itemTypeID INTEGER PRIMARY KEY, typeName TEXT);
CREATE TABLE items (itemID INTEGER PRIMARY KEY, itemTypeID INTEGER, key TEXT, dateAdded TEXT, dateModified TEXT);
CREATE TABLE deletedItems (itemID INTEGER PRIMARY KEY);
CREATE TABLE fields (fieldID INTEGER PRIMARY KEY, fieldName TEXT);
CREATE TABLE itemDataValues (valueID INTEGER PRIMARY KEY, value TEXT);
CREATE TABLE itemData (itemID INTEGER, fieldID INTEGER, valueID INTEGER);
CREATE TABLE creators (creatorID INTEGER PRIMARY KEY, firstName TEXT, lastName TEXT, fieldMode INTEGER);
CREATE TABLE creatorTypes (creatorTypeID INTEGER PRIMARY KEY, creatorType TEXT);
CREATE TABLE itemCreators (itemID INTEGER, creatorID INTEGER, creatorTypeID INTEGER, orderIndex INTEGER);
CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT);
CREATE TABLE itemTags (itemID INTEGER, tagID INTEGER, type INTEGER);
CREATE TABLE collections (collectionID INTEGER PRIMARY KEY, collectionName TEXT, key TEXT, parentCollectionID INTEGER);
CREATE TABLE collectionItems (collectionID INTEGER, itemID INTEGER);
CREATE TABLE itemAttachments (itemID INTEGER PRIMARY KEY, parentItemID INTEGER, path TEXT);
CREATE TABLE itemNotes (itemID INTEGER PRIMARY KEY, parentItemID INTEGER, note TEXT, title TEXT);
CREATE TABLE itemAnnotations (itemID INTEGER PRIMARY KEY, parentItemID INTEGER, type INTEGER,
                              authorName TEXT, text TEXT, comment TEXT, color TEXT,
                              pageLabel TEXT, sortIndex TEXT, position TEXT);
INSERT INTO itemTypes VALUES (1,'journalArticle'),(2,'attachment'),(3,'note'),(4,'annotation');
INSERT INTO fields VALUES (1,'title');
INSERT INTO items VALUES (1,1,'SMOKEZ01','2020-01-01','2020-01-02');
INSERT INTO itemDataValues VALUES (1,'Smoke import');
INSERT INTO itemData VALUES (1,1,1);
INSERT INTO collections VALUES (1,'Smoke collection','SMOKEC01',NULL);
INSERT INTO collectionItems VALUES (1,1);
INSERT INTO items VALUES (2,2,'SMOKEF01','2020-01-01','2020-01-02');
INSERT INTO itemAttachments VALUES (2,1,'storage:smoke.pdf');
INSERT INTO items VALUES (3,3,'SMOKEN01','2020-01-01','2020-01-02');
INSERT INTO itemNotes VALUES (3,1,'<p>A smoky thought.</p>','Note');
INSERT INTO items VALUES (4,4,'SMOKEA01','2020-01-01','2020-01-02');
INSERT INTO itemAnnotations VALUES (4,2,1,'Reader','a highlighted passage','worth rereading',
                                    '#5fb236','7','00001',
                                    '{"pageIndex":6,"rects":[[72,700,300,712]]}');
''')
db.commit()
PYEOF
check "import preview"    "$(j -X POST "$BASE/import/zotero/preview" -d "{\"path\":\"$ZDB\"}" | jq -r .items)"
# `total` is what the import read, which is the same on every run; `items` is
# what was new, which is zero from the second run onwards against a database
# that persists between smoke runs.
check "import commits"    "$(j -X POST "$BASE/libraries/$LIB/import/zotero" -d "{\"path\":\"$ZDB\"}" \
                             | jq -r 'select(.failed == 0) | .total')"
# Running it again updates rather than duplicating; that is why keys are kept.
# Phrased so a wrong answer is empty, not merely a different word — `check`
# passes anything non-empty, which once made "failed" read as a success.
check "import repeatable" "$(j -X POST "$BASE/libraries/$LIB/import/zotero" -d "{\"path\":\"$ZDB\"}" \
                             | jq -r 'select(.failed == 0 and .updated > 0) | "updated \(.updated)"')"
check "import untouched"  "$(test -f "${ZDB}-wal" && echo "journal left" || echo untouched)"
# A highlight belongs to the file it was drawn on, so it must arrive as a child
# of the attachment — and with a palette name, not Zotero's hex, or it would not
# follow the user's theme.
check "import highlights" "$(j "$BASE/libraries/$LIB/items/SMOKEF01/children" \
                             | jq -r '.[] | select(.itemType == "annotation")
                                          | select(.annotationColor == "green")
                                          | .annotationText')"
check "import notes"      "$(j "$BASE/libraries/$LIB/items/SMOKEZ01/children" \
                             | jq -r '.[] | select(.itemType == "note") | .note')"
rm -rf "$(dirname "$ZDB")"

echo "▸ tag colour"
j -X POST "$BASE/libraries/$LIB/items" \
  -d '[{"itemType":"journalArticle","title":"Colour smoke","tags":[{"tag":"colour-smoke"}]}]' >/dev/null
check "tag colour set"    "$(j -X POST "$BASE/libraries/$LIB/tags/color" \
                             -d '{"name":"colour-smoke","color":"violet"}' | jq -r '.color')"
check "tag colour listed" "$(j "$BASE/libraries/$LIB/tags?q=colour-smoke" \
                             | jq -r '.[] | select(.name == "colour-smoke") | .color')"
# Clearing is not "no colour": the client derives one from the name.
check "tag colour clears" "$(j -X POST "$BASE/libraries/$LIB/tags/color" \
                             -d '{"name":"colour-smoke","color":""}' >/dev/null; \
                             j "$BASE/libraries/$LIB/tags?q=colour-smoke" \
                             | jq -r '.[] | select(.name == "colour-smoke") | select(.color == null) | "cleared"')"

echo "▸ browser connector"
# The extension knows only Zotero's paths and holds no key, so these live
# outside /api and outside the guard. They are checked on the main port; the
# second listener is opt-in and this test does not assume it.
CONN="${1:-http://127.0.0.1:23130}"
check "connector ping"    "$(j -X POST "$CONN/connector/ping" -d '{}' | jq -r '.prefs.downloadAssociatedFiles')"
check "connector library" "$(j -X POST "$CONN/connector/getSelectedCollection" -d '{}' | jq -r '.libraryEditable')"
# The connector treats anything but 201 as a save worth retrying.
check "connector saves"   "$(curl -sS -o /dev/null -w '%{http_code}' -X POST "$CONN/connector/saveItems" \
                             -H 'Content-Type: application/json' \
                             -d '{"sessionID":"smoke","items":[{"itemType":"journalArticle","title":"Connector smoke","tags":[{"tag":"smoke-connector"}]}]}' \
                             | grep -x 201)"
# What it saved must be a real item, findable the ordinary way.
check "connector stored"  "$(j "$BASE/libraries/$LIB/items?q=Connector%20smoke&limit=1" \
                             | jq -r '.items[0].title // empty')"
# A translator's tags are automatic, not the user's own.
check "connector tags"    "$(j "$BASE/libraries/$LIB/items?q=Connector%20smoke&limit=1" \
                             | jq -r '.items[0].tags[0] | select(.type == 1) | .tag')"
check "connector session" "$(curl -sS -o /dev/null -w '%{http_code}' -X POST "$CONN/connector/updateSession" \
                             -H 'Content-Type: application/json' -d '{"sessionID":"smoke"}' | grep -x 200)"

echo "▸ graph"
GA=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d '[{"itemType":"journalArticle","title":"Graph focus","tags":[{"tag":"graph-smoke"}]}]' \
       | jq -r '.created[0].key')
j -X POST "$BASE/libraries/$LIB/items" \
  -d '[{"itemType":"journalArticle","title":"Graph neighbour","tags":[{"tag":"graph-smoke"}]}]' >/dev/null
# The focus is always in the picture, and exactly once.
check "graph focus"       "$(j "$BASE/libraries/$LIB/graph/$GA" \
                             | jq -r '[.nodes[] | select(.focus == true)] | length | select(. == 1)')"
# An edge must say why it exists; an unexplained one is a claim on trust.
check "graph tag edge"    "$(j "$BASE/libraries/$LIB/graph/$GA" \
                             | jq -r '.edges[] | select(.relation == "tag") | .target')"
check "graph names nodes" "$(j "$BASE/libraries/$LIB/graph/$GA" \
                             | jq -r '.nodes[] | select(.focus != true) | .title')"
check "graph unknown key" "$(j "$BASE/libraries/$LIB/graph/ZZZZZZZZ" | jq -r '.title // empty')"

echo "▸ references"
# Citations are stored, not derived: they come from the publisher and exist
# whether or not either paper is here. Fetching needs the network, so the
# storage is exercised directly and the fetch is only checked for its refusal.
check "refs need a doi"   "$(j -X POST "$BASE/libraries/$LIB/items/$GA/citations/fetch" -d '{}' \
                             | jq -r '.title // empty | select(contains("DOI"))')"
# `check` passes any non-empty string, so this is phrased so that a wrong
# answer is empty rather than merely a different number.
check "harvest idle"      "$(j "$BASE/libraries/$LIB/citations/harvest" \
                             | jq -r 'select(.running == false) | "idle"')"
# Two runs would only get the client throttled by the service they share.
check "harvest one only"  "$(j -X POST "$BASE/libraries/$LIB/citations/harvest" >/dev/null; \
                             j -X POST "$BASE/libraries/$LIB/citations/harvest" \
                             | jq -r '.title // .message // empty')"
j -X POST "$BASE/libraries/$LIB/citations/harvest/stop" >/dev/null
check "gaps listed"       "$(j "$BASE/libraries/$LIB/citations/missing" \
                             | jq -r 'select((.works | type) == "array") | "listed"')"
check "refs both ways"    "$(j "$BASE/libraries/$LIB/items/$GA/citations" \
                             | jq -r 'select((.cites | type) == "array" and
                                             (.citedBy | type) == "array") | "both"')"

echo "▸ citations"
CK=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d '[{"itemType":"journalArticle","title":"Citation smoke","publicationTitle":"Journal of Smoke","volume":"30","issue":"1","pages":"1-9","date":"2017-06-12","DOI":"10.1000/smoke","creators":[{"creatorType":"author","firstName":"Ashish","lastName":"Vaswani"}]}]' \
       | jq -r '.created[0].key')
check "citation styles"   "$(j "$BASE/citation-styles" | jq -r '.[] | select(.id == "gb7714") | .name')"
# Phrased so a wrong style is empty: APA is the only style here that writes the
# year in brackets straight after an inverted name.
check "renders apa"       "$(j -X POST "$BASE/libraries/$LIB/citations" \
                             -d "{\"keys\":[\"$CK\"],\"style\":\"apa\"}" \
                             | jq -r '.bibliography[0] | select(startswith("Vaswani, A. (2017)."))')"
# A numeric style numbers by position, and must not glue a stop to the DOI.
check "renders ieee"      "$(j -X POST "$BASE/libraries/$LIB/citations" \
                             -d "{\"keys\":[\"$CK\"],\"style\":\"ieee\"}" \
                             | jq -r '.bibliography[0] | select(startswith("[1] A. Vaswani"))
                                                       | select(endswith("10.1000/smoke"))')"
check "renders gb7714"    "$(j -X POST "$BASE/libraries/$LIB/citations" \
                             -d "{\"keys\":[\"$CK\"],\"style\":\"gb7714\"}" \
                             | jq -r '.bibliography[0] | select(contains("VASWANI A. Citation smoke[J]"))')"
check "in-text citation"  "$(j -X POST "$BASE/libraries/$LIB/citations" \
                             -d "{\"keys\":[\"$CK\"],\"style\":\"apa\"}" \
                             | jq -r '.citations[0] | select(. == "(Vaswani, 2017)")')"
check "html italics"      "$(j -X POST "$BASE/libraries/$LIB/citations" \
                             -d "{\"keys\":[\"$CK\"],\"style\":\"apa\",\"format\":\"html\"}" \
                             | jq -r '.bibliography[0] | select(contains("<i>Journal of Smoke</i>"))')"
check "unknown style"     "$(j -X POST "$BASE/libraries/$LIB/citations" \
                             -d "{\"keys\":[\"$CK\"],\"style\":\"nonesuch\"}" \
                             | jq -r '.title // empty | select(contains("nonesuch"))')"

echo "▸ agent"
check "agent status"     "$(j "$BASE/agent" | jq -r 'has("configured")')"

# The agent needs a model, which most environments will not have. Skipping is
# honest; pretending the path is covered when it never ran would not be.
if [[ "$(j "$BASE/agent" | jq -r .configured)" == "true" ]]; then
  AK=$(j -X POST "$BASE/libraries/$LIB/items" \
         -d '[{"itemType":"journalArticle","title":"Agent smoke","abstractNote":"A study of nothing in particular, conducted carefully."}]' \
       | jq -r '.created[0].key')
  # An agent that can change the library must be able to prove it did.
  check "agent tools"    "$(j "$BASE/agent" | jq -r '.tools // empty | length | tostring | select(. != "0")')"
  check "summarise"      "$(j -X POST "$BASE/libraries/$LIB/items/$AK/summarise" -d '{}' | jq -r '.note.itemType')"
  check "summary is a child" "$(j "$BASE/libraries/$LIB/items/$AK/children" | jq -r 'length')"
  # Re-running must replace the note, not add a second one.
  j -X POST "$BASE/libraries/$LIB/items/$AK/summarise" -d '{}' > /dev/null
  check "summary replaced" "$(j "$BASE/libraries/$LIB/items/$AK/children" | jq -r 'if length == 1 then "one" else "duplicated" end')"

  ACONV=$(j -X POST "$BASE/libraries/$LIB/conversations" -d '{"title":"smoke"}' | jq -r .key)
  check "agent answers"  "$(j -X POST "$BASE/libraries/$LIB/conversations/$ACONV/ask" \
                              -d '{"content":"How many items are in the library? Use your tools."}' \
                            | jq -r '.message.content | length > 0')"
  j -X DELETE "$BASE/libraries/$LIB/conversations/$ACONV" > /dev/null
else
  printf '  \033[33mskip\033[0m %-44s %s\n' "agent round trip" "no model configured"
fi

echo "▸ badges"
check "badge columns"    "$(j "$BASE/badges" | jq -r 'length')"
BKEY=$(j -X POST "$BASE/libraries/$LIB/items" \
         -d '[{"itemType":"journalArticle","title":"Badge smoke","ISSN":"0028-0836"}]' \
       | jq -r '.created[0].key')
check "badge resolve"    "$(j -X POST "$BASE/libraries/$LIB/badges" -d "{\"keys\":[\"$BKEY\"]}" \
                            | jq -r ".\"$BKEY\" | length")"

check "badge sort"       "$(j "$BASE/libraries/$LIB/items?sort=badge:journal-metrics:if&direction=asc&limit=1" \
                            | jq -r '.items | length')"
check "badge sort junk"  "$(j "$BASE/libraries/$LIB/items?sort=badge:&limit=1" | jq -r '.items | length')"
check "paging offset"    "$(j "$BASE/libraries/$LIB/items?limit=1&offset=1" | jq -r '.items[0].key')"

echo "▸ conversations"
CONV=$(j -X POST "$BASE/libraries/$LIB/conversations" -d '{"title":"smoke"}' | jq -r .key)
check "conversation"     "$CONV"
check "message append"   "$(j -X POST "$BASE/libraries/$LIB/conversations/$CONV/messages" \
                              -d '{"role":"user","content":"hello"}' | jq -r .role)"
check "transcript"       "$(j "$BASE/libraries/$LIB/conversations/$CONV/messages" | jq -r 'length')"
check "conversation list" "$(j "$BASE/libraries/$LIB/conversations" | jq -r '.[0].messageCount')"
check "conversation drop" "$(j -X DELETE "$BASE/libraries/$LIB/conversations/$CONV" | jq -r .deleted)"

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
