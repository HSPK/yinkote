#!/usr/bin/env bash
# End-to-end smoke test against a running server.
# Usage: scripts/smoke.sh [base-url]
set -uo pipefail

BASE="${1:-http://127.0.0.1:23130}/api/v1"
PASS=0
FAIL=0
# Names of the checks that failed, so the summary says *which* — scrolled-off
# output cost a real investigation once.
FAILED=()

check() { # check <name> <value>
  if [[ -n "${2:-}" && "$2" != "null" && "$2" != "false" && "$2" != "0" ]]; then
    printf '  \033[32mok\033[0m   %-44s %s\n' "$1" "$2"
    PASS=$((PASS + 1))
  else
    printf '  \033[31mFAIL\033[0m %-44s %s\n' "$1" "${2:-<empty>}"
    FAIL=$((FAIL + 1))
    FAILED+=("$1")
  fi
}

skip() { printf '  \033[33mskip\033[0m %-44s %s\n' "$1" "$2"; }

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
# A tag unique to this run. Sharing one across runs meant the suite's own
# accumulated history eventually crossed the "this tag is too common to mean
# anything" threshold — 54 items after twenty-odd runs against a 50 floor — and
# the check started failing while the product was behaving exactly as designed.
#
# A smoke test that accumulates state ends up testing the accumulation.
GTAG="graph-smoke-$RANDOM$RANDOM"
GA=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d "[{\"itemType\":\"journalArticle\",\"title\":\"Graph focus\",\"tags\":[{\"tag\":\"$GTAG\"}]}]" \
       | jq -r '.created[0].key')
j -X POST "$BASE/libraries/$LIB/items" \
  -d "[{\"itemType\":\"journalArticle\",\"title\":\"Graph neighbour\",\"tags\":[{\"tag\":\"$GTAG\"}]}]" >/dev/null
# The focus is always in the picture, and exactly once.
check "graph focus"       "$(j "$BASE/libraries/$LIB/graph/$GA" \
                             | jq -r '[.nodes[] | select(.focus == true)] | length | select(. == 1)')"
# An edge must say why it exists; an unexplained one is a claim on trust.
check "graph tag edge"    "$(j "$BASE/libraries/$LIB/graph/$GA" \
                             | jq -r '.edges[] | select(.relation == "tag") | .target')"
check "graph names nodes" "$(j "$BASE/libraries/$LIB/graph/$GA" \
                             | jq -r '.nodes[] | select(.focus != true) | .title')"
check "graph unknown key" "$(j "$BASE/libraries/$LIB/graph/ZZZZZZZZ" | jq -r '.title // empty')"

# Citation edges over the real HTTP path. Unique DOIs per run for the same
# reason the tag above is unique: a suite that reuses identifiers ends up
# testing what earlier runs left behind.
GDOI="10.5555/smoke$RANDOM$RANDOM"
CA=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d "[{\"itemType\":\"journalArticle\",\"title\":\"Cited focus\",\"DOI\":\"$GDOI-a\"}]" \
     | jq -r '.created[0].key')
CB=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d "[{\"itemType\":\"journalArticle\",\"title\":\"Cited partner\",\"DOI\":\"$GDOI-b\"}]" \
     | jq -r '.created[0].key')
# Two papers citing the same three works: one would be a bibliography, not a
# pattern, and both edges are defined to need two.
for n in 1 2; do
  CITER=$(j -X POST "$BASE/libraries/$LIB/items" \
            -d "[{\"itemType\":\"journalArticle\",\"title\":\"Citing paper $n\"}]" \
          | jq -r '.created[0].key')
  j -X PUT "$BASE/libraries/$LIB/items/$CITER/citations" \
    -d "{\"citations\":[{\"doi\":\"$GDOI-a\",\"label\":\"A\"},{\"doi\":\"$GDOI-b\",\"label\":\"B\"},{\"doi\":\"$GDOI-c\",\"label\":\"C\"}]}" > /dev/null
done
# A recorded reference must resolve to the item the library holds; writing a
# fingerprint by hand is exactly where that goes wrong.
check "citations recorded" "$(j "$BASE/libraries/$LIB/items/$CITER/citations" \
                             | jq -r '.resolved | select(. == 2)')"
# The assistant must be able to read a bibliography, not only count it: a
# code comment referred to this tool for a round before it existed.
check "reference tool"    "$(j "$BASE/agent" | jq -r '.tools | map(select(. == "list_references")) | length | select(. == 1)')"
check "graph coupling"    "$(j "$BASE/libraries/$LIB/graph/$CITER" \
                             | jq -r '[.edges[] | select(.relation == "coupling")] | length | select(. > 0)')"
check "graph cocitation"  "$(j "$BASE/libraries/$LIB/graph/$CA" \
                             | jq -r --arg k "$CB" '[.edges[] | select(.relation == "cocitation" and .target == $k)] | length | select(. == 1)')"

echo "▸ export"
EXKEY=$(j -X POST "$BASE/libraries/$LIB/items" \
          -d '{"itemType":"journalArticle","title":"Exported 100% {Braced} Paper","date":"2018","publicationTitle":"Journal of Tests","pages":"10-20","DOI":"10.1/exp","creators":[{"creatorType":"author","lastName":"Ito","firstName":"Ken"}]}' \
          | jq -r '.created[0].key')
BIB=$(curl -sS -H 'Content-Type: application/json' -X POST "$BASE/libraries/$LIB/export" \
        -d "$(jq -nc --arg k "$EXKEY" '{itemKeys:[$k],format:"bibtex"}')")
check "bibtex entry"      "$(echo "$BIB" | grep -o '^@article{ito2018exported,' | head -1)"
check "bibtex pages"      "$(echo "$BIB" | grep -o 'pages = {10--20}')"
# The characters that break the *file* rather than the entry.
check "bibtex escapes"    "$(echo "$BIB" | grep -c 'Exported 100\\% \\{Braced\\}' | grep -v '^0$')"
check "bibtex balanced"   "$(echo "$BIB" | python3 -c "
import sys
text = sys.stdin.read()
# Only unescaped braces count towards balance.
stripped = text.replace('\\\\{', '').replace('\\\\}', '')
print('balanced' if stripped.count('{') == stripped.count('}') else '')
")"
RIS=$(curl -sS -H 'Content-Type: application/json' -X POST "$BASE/libraries/$LIB/export" \
        -d "$(jq -nc --arg k "$EXKEY" '{itemKeys:[$k],format:"ris"}')")
check "ris terminated"    "$(echo "$RIS" | grep -q '^TY  - JOUR$' && echo "$RIS" | grep -q '^ER  - *$' && echo "well formed")"
CSL=$(curl -sS -H 'Content-Type: application/json' -X POST "$BASE/libraries/$LIB/export" \
        -d "$(jq -nc --arg k "$EXKEY" '{itemKeys:[$k],format:"csljson"}')")
check "csl json parses"   "$(echo "$CSL" | jq -r '.[0] | select(.type == "article-journal") | .author[0].family')"
check "csl json year"     "$(echo "$CSL" | jq -r '.[0].issued["date-parts"][0][0] | select(. == 2018)')"
check "export refuses"    "$(curl -sS -o /dev/null -w '%{http_code}' -H 'Content-Type: application/json' \
                              -X POST "$BASE/libraries/$LIB/export" \
                              -d "$(jq -nc --arg k "$EXKEY" '{itemKeys:[$k],format:"zotero-rdf"}')" \
                              | grep -q '^4' && echo "rejected")"

echo "▸ bibliography import"
# The round trip through the API: what the server just wrote, it can read back.
# This is what catches an escaping bug in either direction, and the title is
# chosen to contain everything that breaks a BibTeX file.
IMP=$(j -X POST "$BASE/libraries/$LIB/import/bibliography" \
        -d "$(jq -nc --arg t "$BIB" '{text:$t}')")
check "imported one"      "$(echo "$IMP" | jq -r '.imported | select(. == 1)')"
check "nothing skipped"   "$(echo "$IMP" | jq -r '.skipped | select(. == 0) | "clean"')"
IMPKEY=$(echo "$IMP" | jq -r '.keys[0]')
check "title survived"    "$(j "$BASE/libraries/$LIB/items/$IMPKEY" \
                             | jq -r '.title | select(. == "Exported 100% {Braced} Paper")')"
check "author survived"   "$(j "$BASE/libraries/$LIB/items/$IMPKEY" \
                             | jq -r '.creators[0] | select(.lastName == "Ito" and .firstName == "Ken") | "Ito, Ken"')"
check "pages survived"    "$(j "$BASE/libraries/$LIB/items/$IMPKEY" | jq -r '.pages | select(. == "10-20")')"
# RIS too, from the file the server wrote a moment ago.
RIMP=$(j -X POST "$BASE/libraries/$LIB/import/bibliography" \
         -d "$(jq -nc --arg t "$RIS" '{text:$t}')")
check "ris imported"      "$(echo "$RIMP" | jq -r '.imported | select(. == 1)')"
# A file of rubbish is reported, not accepted and not fatal.
check "rubbish reported"  "$(j -X POST "$BASE/libraries/$LIB/import/bibliography" \
                             -d '{"text":"@article{broken, year = {2019} }"}' \
                             | jq -r 'select(.imported == 0 and .skipped == 1) | .reasons[0]')"

echo "▸ maintenance"
# Asked, not assumed: the server knows where it keeps its data, and a script
# that hard-codes the path checks a different machine's backups on the day
# somebody runs it with a different --data-dir.
DATA=$(j "$BASE/ping" | jq -r .dataDir)
# A backup is worth what can be restored from it, so the check is that the file
# opens as a library and holds the same number of items — not that the endpoint
# returned 200.
BK=$(j -X POST "$BASE/maintenance/backup")
BKNAME=$(echo "$BK" | jq -r .name)
check "backup taken"      "$(echo "$BK" | jq -r '.bytes | select(. > 0) | "written"')"
check "backup named"      "$(echo "$BKNAME" | grep -qE '^yinkote-[0-9]{8}\.db$' && echo "$BKNAME")"
# Named rather than counted. A count has to agree about what it is counting —
# trashed items, child items — and comparing two slightly different questions
# is how this check first failed against a backup that was perfectly good. An
# item created earlier in this run either made it into the copy or did not.
check "backup restores"   "$(python3 - "$DATA/backups/$BKNAME" "$KEY" <<'PY'
import sqlite3, sys
db = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
ok = list(db.execute("PRAGMA integrity_check"))[0][0]
found = list(db.execute("SELECT count(*) FROM items WHERE key = ?", (sys.argv[2],)))[0][0]
total = list(db.execute("SELECT count(*) FROM items"))[0][0]
print("restored" if ok == "ok" and found == 1 and total > 0 else "")
PY
)"
# Taking one twice in a day replaces it rather than failing.
check "backup repeats"    "$(j -X POST "$BASE/maintenance/backup" | jq -r --arg n "$BKNAME" '.name | select(. == $n)')"
check "backups listed"    "$(j "$BASE/maintenance/backups" | jq -r --arg n "$BKNAME" '[.backups[] | select(.name == $n)] | length | select(. == 1)')"
check "integrity checked" "$(j "$BASE/maintenance/integrity" | jq -r '.checked | tostring | select(. != "null")')"
check "integrity reports" "$(j "$BASE/maintenance/integrity" | jq -r 'select((.missing | type) == "array" and (.orphans | type) == "array") | "both directions"')"

echo "▸ duplicates"
# Two records of one paper, one of which carries the PDF.
# Unique to this run. The smoke database is kept between runs, so a fixed
# title would make this run's pair a duplicate of every previous run's pair,
# and "the group is gone" could never come true again.
DUPT="A Duplicated Paper $$-$(date +%s)"
DA=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d "$(jq -nc --arg t "$DUPT" '{itemType:"journalArticle",title:$t,date:"2019",creators:[{creatorType:"author",lastName:"Kim"}]}')" \
       | jq -r '.created[0].key')
DB=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d "$(jq -nc --arg t "$DUPT" '{itemType:"journalArticle",title:($t|ascii_downcase),date:"2019",creators:[{creatorType:"author",lastName:"Kim"}],DOI:"10.5555/dup-\($t|@base64)"}')" \
       | jq -r '.created[0].key')
j -X POST "$BASE/libraries/$LIB/items" \
  -d "{\"itemType\":\"attachment\",\"parentKey\":\"$DB\",\"contentType\":\"application/pdf\",\"linkMode\":\"imported_file\"}" >/dev/null
check "duplicate found"   "$(j "$BASE/libraries/$LIB/duplicates" \
                             | jq -r --arg a "$DA" '[.groups[] | select(any(.[]; .key == $a))] | length | select(. == 1)')"
# Keep the thin record, so the merge has to carry the other one's PDF across.
check "merged"            "$(j -X POST "$BASE/libraries/$LIB/items/merge" \
                             -d "{\"master\":\"$DA\",\"others\":[\"$DB\"]}" \
                             | jq -r '.merged | select(. == 1)')"
check "kept the pdf"      "$(j "$BASE/libraries/$LIB/items/$DA" | jq -r '.attachments | join("+") | select(. == "pdf")')"
check "filled the gap"    "$(j "$BASE/libraries/$LIB/items/$DA" | jq -r '.DOI | select(startswith("10.5555/dup-"))')"
# Recoverable: the loser is in the trash, not destroyed.
check "loser is trashed"  "$(j "$BASE/libraries/$LIB/items/$DB" | jq -r '.deleted | select(. == true) | "trashed"')"
check "group is gone"     "$(j "$BASE/libraries/$LIB/duplicates" \
                             | jq -r --arg a "$DA" '[.groups[] | select(any(.[]; .key == $a))] | length | select(. == 0) | "resolved"')"

echo "▸ word integration"
# The protocol every word processor uses. The point of the whole design is that
# inserting a citation renumbers the ones after it, so that is what is checked.
WA=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d '{"itemType":"journalArticle","title":"Alpha","date":"2020","creators":[{"creatorType":"author","lastName":"Zhang"}]}' \
       | jq -r '.created[0].key')
WB=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d '{"itemType":"journalArticle","title":"Beta","date":"2021","creators":[{"creatorType":"author","lastName":"Li"}]}' \
       | jq -r '.created[0].key')
SID=$(j -X POST "$BASE/integration/session" \
        -d '{"docId":"smoke.docx","docPrefs":{"styleId":"ieee","format":"text"}}' | jq -r .sessionId)
check "session opened"    "$SID"
# Reopening the same document must land on the same session rather than leak one.
check "session is stable" "$(j -X POST "$BASE/integration/session" \
                              -d '{"docId":"smoke.docx"}' | jq -r --arg s "$SID" '.sessionId | select(. == $s)')"

SNAP="{\"fieldsSnapshot\":[{\"id\":\"f1\",\"text\":\"\",\"citation\":{\"keys\":[\"$WB\"]}},{\"id\":\"f2\",\"text\":\"[1]\",\"citation\":{\"keys\":[\"$WA\"]}}]}"
check "citation renumbers" "$(j -X POST "$BASE/integration/session/$SID/cite" -d "$SNAP" \
                              | jq -r '[.updatedFields[].text] | join("") | select(. == "[1][2]")')"
check "bibliography order" "$(j -X POST "$BASE/integration/session/$SID/bibliography" -d "$SNAP" \
                              | jq -r --arg b "$WB" '.entries[0].key | select(. == $b)')"
# A settled document must not report a single change, or every refresh would
# dirty the whole file.
SETTLED="{\"fieldsSnapshot\":[{\"id\":\"f1\",\"text\":\"[1]\",\"citation\":{\"keys\":[\"$WB\"]}},{\"id\":\"f2\",\"text\":\"[2]\",\"citation\":{\"keys\":[\"$WA\"]}}]}"
check "refresh is quiet"  "$(j -X POST "$BASE/integration/session/$SID/refresh" -d "$SETTLED" \
                              | jq -r '.updatedFields | length | select(. == 0) | "no changes"')"
# Changing the style invalidates every citation, and the add-in is told so.
check "style change"      "$(j -X PUT "$BASE/integration/session/$SID/prefs" \
                              -d '{"styleId":"apa","format":"text"}' | jq -r '.refreshRequired')"
check "restyled citation" "$(j -X POST "$BASE/integration/session/$SID/refresh" -d "$SETTLED" \
                              | jq -r '[.updatedFields[].text] | join(" ") | select(test("Li, 2021"))')"
check "session closed"    "$(j -X POST "$BASE/integration/session/$SID/close" | jq -r .closed)"
check "closed stays shut" "$(j -X POST "$BASE/integration/session/$SID/refresh" -d "$SNAP" \
                              | jq -r '.error.kind // .error // "rejected"' | head -c 20)"

echo "▸ attachment marks"
# A listed row must say what it has attached without a second request: the
# table draws a glyph per kind, and a per-row lookup for a hundred rows is the
# thing this replaced.
MARKED=$(j -X POST "$BASE/libraries/$LIB/items" \
           -d '{"itemType":"journalArticle","title":"Row with a PDF"}' | jq -r '.created[0].key')
j -X POST "$BASE/libraries/$LIB/items" \
  -d "{\"itemType\":\"attachment\",\"parentKey\":\"$MARKED\",\"contentType\":\"application/pdf\",\"linkMode\":\"imported_file\"}" >/dev/null
j -X POST "$BASE/libraries/$LIB/items" \
  -d "{\"itemType\":\"attachment\",\"parentKey\":\"$MARKED\",\"contentType\":\"text/html\",\"linkMode\":\"linked_url\"}" >/dev/null
# Ordered by how telling the kind is, so a row leads with its PDF.
check "marks on the row"  "$(j "$BASE/libraries/$LIB/items/$MARKED" \
                             | jq -r '.attachments | join("+") | select(. == "pdf+link")')"
check "marks in the list" "$(j "$BASE/libraries/$LIB/items?limit=200" \
                             | jq -r --arg k "$MARKED" '.items[] | select(.key == $k) | .attachments | join("+") | select(. == "pdf+link")')"
# Absent, not empty: a row with nothing attached should not carry the key.
check "no marks, no key"  "$(j "$BASE/libraries/$LIB/items/$KEY" \
                             | jq -r 'select(has("attachments") | not) | "absent"')"

echo "▸ file browser"
check "files listed"      "$(j "$BASE/libraries/$LIB/files" \
                             | jq -r 'select((.files | type) == "array") | "listed"')"
# A file's address is what a file browser is opened to find out.
check "files keep source" "$(j "$BASE/libraries/$LIB/files" \
                             | jq -r '[.files[] | select(.url != "")] | length | tostring
                                      | select(. != "0")' 2>/dev/null || echo "none stored yet")"
# Preview must change nothing, which is the whole reason it exists.
BEFORE=$(j "$BASE/libraries/$LIB/files" | jq -r '[.files[].filename] | @csv')
check "preview is silent" "$(j -X POST "$BASE/libraries/$LIB/files/preview" \
                             -d '{"template":"{author} {year} - {title}"}' >/dev/null; \
                             AFTER=$(j "$BASE/libraries/$LIB/files" | jq -r '[.files[].filename] | @csv'); \
                             [[ "$BEFORE" == "$AFTER" ]] && echo unchanged)"
check "preview explains"  "$(j -X POST "$BASE/libraries/$LIB/files/preview" \
                             -d '{"template":"{author} {year} - {title}"}' \
                             | jq -r 'select(.template != null and .total != null) | "planned"')"
# The count is the answer; the rows are only evidence. Sending every one of
# them was 3.7 MB for a panel that shows eight lines.
check "preview is small"  "$(j -X POST "$BASE/libraries/$LIB/files/preview" \
                             -d '{"template":"{author} {year} - {title}"}' \
                             | jq -r 'select((.changes | length) <= 50) | "sampled"')"

echo "▸ download queue"
DK=$(j -X POST "$BASE/libraries/$LIB/items" \
       -d '[{"itemType":"journalArticle","title":"Queue smoke"}]' | jq -r '.created[0].key')
check "queue accepts many" "$(j -X POST "$BASE/libraries/$LIB/downloads" \
                              -d "{\"itemKey\":\"$DK\",\"urls\":[\"https://example.invalid/a.pdf\",\"https://example.invalid/b.pdf\"]}" \
                              | jq -r 'select(.queued == 2) | "two"')"
# Asking twice for one file is one request, not two.
check "queue dedupes"      "$(j -X POST "$BASE/libraries/$LIB/downloads" \
                              -d "{\"itemKey\":\"$DK\",\"urls\":[\"https://example.invalid/a.pdf\"]}" \
                              | jq -r 'select(.queued == 1) | "same row"')"
check "queue is readable"  "$(j "$BASE/libraries/$LIB/downloads" \
                              | jq -r '.downloads | length | tostring | select(. != "0")')"
# An unreachable host must fail visibly and stay retryable, not vanish.
sleep 6
check "failure is kept"    "$(j "$BASE/libraries/$LIB/downloads" \
                              | jq -r '[.downloads[] | select(.state == "failed") | .error][0]
                                       | select(length > 0) | "explained"')"
FID=$(j "$BASE/libraries/$LIB/downloads" | jq -r '[.downloads[] | select(.state == "failed")][0].id')
check "failure retries"    "$(j -X POST "$BASE/libraries/$LIB/downloads/retry" \
                              -d "{\"ids\":[$FID]}" | jq -r 'select(.retrying == 1) | "requeued"')"
check "queue removes"      "$(j -X POST "$BASE/libraries/$LIB/downloads/remove" \
                              -d "{\"ids\":[$FID]}" | jq -r 'select(.removed == 1) | "gone"')"
# An item key that does not exist is the caller's mistake, and is told so now
# rather than reported later as a failed download.
check "queue checks item"  "$(j -X POST "$BASE/libraries/$LIB/downloads" \
                              -d '{"itemKey":"ZZZZZZZZ","urls":["https://example.invalid/x.pdf"]}' \
                              | jq -r '.title // empty')"

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
# The count *is* the view. It shipped as `cited_by` against a client reading
# `citedBy`, so the column was blank and nothing failed — a whole feature
# quietly answering nothing.
check "gaps name fields"  "$(j "$BASE/libraries/$LIB/citations/missing" \
                             | jq -r '.works[0] // {"citedBy":0} | has("citedBy")
                                      | select(. == true) | "camelCase"')"
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
# The workbench must be able to point the assistant at a model: this is a
# local server the user started, and "edit a TOML file and restart" would make
# the web interface a partial one. Half a configuration is refused, and the
# key is never handed back.
# Explicitly clearing the endpoint, not merely omitting it: this server may
# already have one from the environment, and a check that quietly *succeeds*
# would overwrite the running configuration and take every later agent check
# down with it. It did, once.
check "half a config refused" "$(j -X PUT "$BASE/agent" -d '{"endpoint":"","model":"only-a-name"}' \
                                  | jq -r 'select(.status == 422) | "refused"')"
check "key is write-only" "$(j "$BASE/agent" | jq -r 'if has("apiKey") then empty else "hidden" end')"

# The agent needs a model, which most environments will not have. Skipping is
# honest; pretending the path is covered when it never ran would not be.
if [[ "$(j "$BASE/agent" | jq -r .configured)" == "true" ]]; then
  AK=$(j -X POST "$BASE/libraries/$LIB/items" \
         -d '[{"itemType":"journalArticle","title":"Agent smoke","abstractNote":"A study of nothing in particular, conducted carefully."}]' \
       | jq -r '.created[0].key')
  # An agent that can change the library must be able to prove it did.
  check "agent tools"    "$(j "$BASE/agent" | jq -r '.tools // empty | length | tostring | select(. != "0")')"
  # Skills and the workspace are separate wirings, and either can go missing
  # without the other noticing; name them rather than trusting the count.
  check "skill tool"     "$(j "$BASE/agent" | jq -r '.tools | index("read_skill") | tostring | select(. != "null")')"
  check "file tools"     "$(j "$BASE/agent" | jq -r '[.tools[] | select(. == "read_file" or . == "write_file" or . == "list_files")] | length | select(. == 3)')"
  # A turn belongs to the conversation, not to the request that started it.
  CK2=$(j -X POST "$BASE/libraries/$LIB/conversations" -d '{"title":"Run smoke"}' | jq -r .key)
  check "ask returns now"  "$(j -X POST "$BASE/libraries/$LIB/conversations/$CK2/ask" \
                               -d '{"content":"how many items are in my library?"}' \
                               | jq -r 'select(.started == true) | "started"')"
  check "run is readable"  "$(j "$BASE/libraries/$LIB/conversations/$CK2/run" \
                               | jq -r 'select((.running | type) == "boolean") | "readable"')"
  # Two turns in one conversation would interleave into a transcript nobody
  # can read, so the second is refused rather than queued.
  check "one turn at once" "$(j -X POST "$BASE/libraries/$LIB/conversations/$CK2/ask" \
                               -d '{"content":"again"}' | jq -r '.title // empty')"
  check "run is stoppable" "$(j -X POST "$BASE/libraries/$LIB/conversations/$CK2/cancel" \
                               | jq -r 'select(.stopping == true) | "stopping"')"
  # A shared endpoint that is busy right now is not a broken feature, and a
  # suite that cannot tell the difference stops being believed. The retry in
  # `yk-ai` waits a rate limit out; when it is still limited after that, these
  # checks are skipped and say so.
  SUMM=$(j -X POST "$BASE/libraries/$LIB/items/$AK/summarise" -d '{}')
  if grep -qiE '429|rate limit' <<< "$SUMM"; then
    THROTTLED=1
    skip "model round trip" "the model is rate-limited right now"
  else
    THROTTLED=0
  fi

  if [[ $THROTTLED -eq 0 ]]; then
  check "summarise"      "$(jq -r '.note.itemType' <<< "$SUMM")"
  check "summary is a child" "$(j "$BASE/libraries/$LIB/items/$AK/children" | jq -r 'length')"
  # Re-running must replace the note, not add a second one.
  j -X POST "$BASE/libraries/$LIB/items/$AK/summarise" -d '{}' > /dev/null
  check "summary replaced" "$(j "$BASE/libraries/$LIB/items/$AK/children" | jq -r 'if length == 1 then "one" else "duplicated" end')"

  ACONV=$(j -X POST "$BASE/libraries/$LIB/conversations" -d '{"title":"smoke"}' | jq -r .key)
  # Starting returns at once now, so the answer is waited for the way a client
  # does: by watching the run, not by holding the request open.
  j -X POST "$BASE/libraries/$LIB/conversations/$ACONV/ask" \
    -d '{"content":"How many items are in the library? Use your tools."}' > /dev/null
  for _ in $(seq 1 60); do
    [[ "$(j "$BASE/libraries/$LIB/conversations/$ACONV/run" | jq -r .running)" == "true" ]] || break
    sleep 1
  done
  check "agent answers"  "$(j "$BASE/libraries/$LIB/conversations/$ACONV/messages" \
                            | jq -r '[.messages[] | select(.role == "assistant")][0].content
                                     | select(length > 0) | "answered"')"
  # A turn that ran must leave its steps behind, or the answer is unverifiable.
  check "agent shows work" "$(j "$BASE/libraries/$LIB/conversations/$ACONV/messages" \
                              | jq -r '[.messages[] | select(.role == "assistant")][0].meta.trace
                                       | select(length > 0) | "traced"')"
  j -X DELETE "$BASE/libraries/$LIB/conversations/$ACONV" > /dev/null
  fi
else
  skip "agent round trip" "no model configured"
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
check "transcript"       "$(j "$BASE/libraries/$LIB/conversations/$CONV/messages" | jq -r '.messages | length')"
check "conversation list" "$(j "$BASE/libraries/$LIB/conversations" | jq -r '.[0].messageCount')"
check "conversation drop" "$(j -X DELETE "$BASE/libraries/$LIB/conversations/$CONV" | jq -r .deleted)"

echo "▸ mentions"
MITEM=$(j -X POST "$BASE/libraries/$LIB/items" \
          -d '[{"itemType":"journalArticle","title":"Mention smoke"}]' | jq -r '.created[0].key')
MCONV=$(j -X POST "$BASE/libraries/$LIB/conversations" -d '{"title":"Mention smoke"}' | jq -r .key)
j -X POST "$BASE/libraries/$LIB/conversations/$MCONV/messages" \
  -d "{\"role\":\"user\",\"content\":\"about this\",\"mentions\":[\"$MITEM\"]}" > /dev/null
# A mention has to survive the round trip, or the chip cannot be drawn.
check "mention stored"   "$(j "$BASE/libraries/$LIB/conversations/$MCONV/messages" \
                             | jq -r --arg k "$MITEM" '.messages[0].mentions | map(select(. == $k)) | length | select(. == 1)')"
# The reverse lookup is what the detail panel asks.
check "paper knows its threads" "$(j "$BASE/libraries/$LIB/items/$MITEM/conversations" \
                             | jq -r --arg c "$MCONV" '.conversations | map(select(.key == $c)) | length | select(. == 1)')"

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
  for name in "${FAILED[@]}"; do
    printf '\033[31m  failed: %s\033[0m\n' "$name"
  done
  exit 1
fi
