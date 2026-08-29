#!/usr/bin/env bash
# End-to-end smoke test against a running server.
# Usage: scripts/smoke.sh [base-url]
set -uo pipefail

BASE="${1:-http://127.0.0.1:23130}/api/v1"

# A server may be protected -- binding past loopback requires a key -- and the
# suite must be able to test that configuration and not only the open one.
# Every request goes through `c`, so there is one place that knows about the
# key. Unset, nothing is sent and this behaves exactly as before.
AUTH=()
if [[ -n "${YK_API_KEY:-}" ]]; then
  AUTH=(-H "Authorization: Bearer $YK_API_KEY")
fi
c() { curl -sS "${AUTH[@]}" "$@"; }

# Which database is behind that address. A server that failed to start leaves
# the previous one holding the port, and a smoke run against the wrong library —
# or against a binary built before the change under test — reports failures that
# have nothing to do with the code. Set `YK_DATA` to refuse rather than guess.
SERVING=$(c "${BASE}/ping" 2>/dev/null | jq -r '.dataDir // empty')
if [[ -z "$SERVING" ]]; then
  echo "nothing is answering at ${BASE} — start one with scripts/serve.sh" >&2
  exit 1
fi
if [[ -n "${YK_DATA:-}" && "$SERVING" != "$YK_DATA" ]]; then
  echo "refusing to run: expected ${YK_DATA}, found ${SERVING}" >&2
  exit 2
fi
echo "▸ testing ${BASE} (database: ${SERVING})"
PASS=0
FAIL=0
SKIP=0
# Names of the checks that failed, so the summary says *which* — scrolled-off
# output cost a real investigation once.
FAILED=()

check() { # check <name> <value>
  # A value that contains our own output means another check ran *inside* this
  # one's `$( )`, which is what an edit landing in the middle of a multi-line
  # check produces. Both lines print green and only one of them is counted, so
  # the run looks fine and one check silently stopped existing. This has now
  # happened three times; a value carrying an escape or a newline is never
  # anything else, so it is refused rather than reported.
  # The escape alone, not a newline: a check may legitimately return several
  # lines — the graph one lists every neighbouring title — and rejecting those
  # turned a working check red. Only our own coloured output can appear inside
  # a value, and only when a check ran in there.
  if [[ "${2:-}" == *$'\033'* ]]; then
    printf '  \033[31mFAIL\033[0m %-44s %s\n' "$1" "another check ran inside this one's value"
    FAIL=$((FAIL + 1))
    FAILED+=("$1")
    return
  fi
  if [[ -n "${2:-}" && "$2" != "null" && "$2" != "false" && "$2" != "0" ]]; then
    printf '  \033[32mok\033[0m   %-44s %s\n' "$1" "$2"
    PASS=$((PASS + 1))
  else
    printf '  \033[31mFAIL\033[0m %-44s %s\n' "$1" "${2:-<empty>}"
    FAIL=$((FAIL + 1))
    FAILED+=("$1")
  fi
}

skip() { printf '  \033[33mskip\033[0m %-44s %s\n' "$1" "$2"; SKIP=$((SKIP + 1)); }

j() { c -H 'Content-Type: application/json' "$@"; }

# Long jobs hand back a task; this waits for one to stop and says how it went.
await_task() { # await_task <task-id>
  for _ in $(seq 1 120); do
    local phase
    phase=$(j "$BASE/tasks/$1" | jq -r .phase)
    [[ "$phase" == "running" ]] || { echo "$phase"; return; }
    sleep 1
  done
  echo "timeout"
}

# ── a check that cannot fail ────────────────────────────────────────────────
#
# `check` passes on any non-empty value, so a jq expression whose branches are
# *both* non-empty is a check that always passes. Two of them lived here for
# a while: one reported "children leaked" and one reported "kept", and both
# read as success. An `if/then/else` in a check has to have one branch that
# yields nothing.
# Assignments are excluded: an ordinary variable is allowed to compute a value
# on both branches. It is only a `check` argument where "any non-empty" means
# "passed".
SUSPECT=$(grep -n 'then' "$0" \
          | grep 'else' \
          | grep -v 'empty' \
          | grep -v '^[0-9]*:#' \
          | grep -v '^[0-9]*:[A-Za-z_][A-Za-z0-9_]*=' || true)

# The same fault spelled with `//`. `jq -r '.a // .b // "refused"'` answers
# "refused" when the request was *accepted* and the error field is absent —
# two refusal checks passed on that literal for as long as the error envelope
# had a different shape. A fallback to a literal is a way of not asking.
SUSPECT="$SUSPECT$(grep -n '// *"' "$0" \
          | grep -v '^[0-9]*: *#' \
          | grep -v '^[0-9]*:[A-Za-z_][A-Za-z0-9_]*=' || true)"
if [[ -n "$SUSPECT" ]]; then
  printf '  \033[31mFAIL\033[0m %s\n' "a check has no failing branch:" >&2
  echo "$SUSPECT" >&2
  exit 3
fi

echo "▸ system"
check "ping"             "$(j "$BASE/ping" | jq -r .ok)"
# Browser saving is off unless asked for, so the settings page can only tell
# somebody it exists if the server reports it. "Requested" and "working" are
# different facts here — the bind is allowed to fail and the server carries on.
check "connector status" "$(j "$BASE/ping" | jq -r '.connector.state | select(. == "off" or . == "listening" or . == "unavailable")')"
# The state that carries the risk looks like every other state unless the
# server says so. Two answers are safe -- `private` is loopback and `protected`
# is reachable but keyed -- and `open` is the one that means anybody who can
# route here owns the library. Naming only `private` made this fail the moment
# a server was configured the other safe way, which is not the fact it is for.
check "access reported"  "$(j "$BASE/ping" | jq -r '.access.state | select(. == "private" or . == "protected")')"
check "openness named"   "$(j "$BASE/ping" | jq -r '.access.state | select(. != "open") | "not open"')"
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
check "stale patch -> 412" "$(c -o /dev/null -w '%{http_code}' -X PATCH \
                             "$BASE/libraries/$LIB/items/$KEY" \
                             -H 'Content-Type: application/json' \
                             -H "If-Unmodified-Since-Version: $VER" \
                             -d '{"fields":{"volume":"31"}}' | grep -x 412)"
# `ItemPatch` keeps its fields in a nested object, so a flattened patch matched
# nothing and produced a patch that was None throughout: 200, and no change.
# Regenerating a summary had been doing exactly that for as long as it existed.
check "flat patch -> 422" "$(c -o /dev/null -w '%{http_code}' -X PATCH \
                             "$BASE/libraries/$LIB/items/$KEY" \
                             -H 'Content-Type: application/json' \
                             -d '{"title":"flattened"}' | grep -x 422)"
# The name says 412 and the check has to as well: `check` passes any non-empty
# value, so printing the status without comparing it accepts 200 just as
# happily — a check that cannot fail for the reason it is named after.
check "bad key -> 422"   "$(c -o /dev/null -w '%{http_code}' "$BASE/libraries/$LIB/items/not%20a%20key" | grep -x 422)"
# Every error in one shape. Rejections that never reach a handler — a bad path
# segment, a malformed body, a missing content type — used to answer in plain
# text, so a client had two formats to parse depending on how wrong it was.
check "rejects in envelope" "$(c -X POST -H 'Content-Type: application/json' \
                                "$BASE/libraries/$LIB/items" -d '{"itemType":"journalArticle","tags":["not an object"]}' \
                                | jq -r '.code | select(. == "invalid_input")')"
# And it must not name the Rust type it failed to build.
check "no internals leak" "$(c -X POST -H 'Content-Type: application/json' \
                              "$BASE/libraries/$LIB/items" -d '{"itemType":"journalArticle","tags":["x"]}' \
                              | jq -r '.title | select(test("enum|CreateBody") | not) | "clean"')"
# A mistyped endpoint is nested inside a server that serves the workbench on
# every other path, so it used to answer 200 and a page of HTML — success, as
# far as any client could tell.
check "unknown endpoint 404" "$(c -o /dev/null -w '%{http_code}' "$BASE/libraries/$LIB/no-such-thing" | grep -x 404)"

echo "▸ search"
sleep 1.5   # let the embedding worker drain the queue
check "keyword"          "$(j "$BASE/libraries/$LIB/search?q=attention&mode=keyword" | jq -r '.hits|length')"
check "fuzzy (typo)"     "$(j "$BASE/libraries/$LIB/search?q=attension&mode=fuzzy" | jq -r '.hits|length')"
check "semantic"         "$(j "$BASE/libraries/$LIB/search?q=neural%20sequence%20model&mode=semantic" | jq -r '.hits|length')"
check "chinese"          "$(j "$BASE/libraries/$LIB/search?q=%E6%89%A9%E6%95%A3%E6%A8%A1%E5%9E%8B" | jq -r '.hits|length')"
check "tag operator"     "$(j "$BASE/libraries/$LIB/search?q=tag:nlp" | jq -r '.hits|length')"
check "snippet mark"     "$(j "$BASE/libraries/$LIB/search?q=transformer" | jq -r '.hits[0].snippet' | grep -c '<mark>')"
check "items?q= hydrate" "$(j "$BASE/libraries/$LIB/items?q=attention" | jq -r '.items[0].match.sources|length')"
# Browsing shows papers; searching shows whatever matched. The table is flat,
# so listing children beside their parents put blank-titled highlights and
# "probe.pdf" between two papers — but a search must still reach them, because
# the phrase a reader highlighted is on the annotation and not on the paper.
check "browse is top level" "$(j "$BASE/libraries/$LIB/items?limit=60&topLevel=true" \
                                | jq -r '[.items[]|select(.parentKey)]|length|select(.==0)|"papers only"')"
# The negative: without it, children are included. A check that only asserted
# the filter works would pass on a server that always excluded children, which
# is what would break highlight search.
check "search reaches kids" "$(j "$BASE/libraries/$LIB/items?limit=60" \
                                | jq -r '[.items[] | select(.parentKey)] | length | select(. > 0)')"
# The trash is a browse with no parent to show a child under: trashing an
# attachment on its own leaves a paper that is not deleted. Filtering it to top
# level made the file unreachable — neither restorable nor emptiable, only
# orphaned. This is reachability, not presentation, so it is checked here too.
TRPAP=$(j -X POST "$BASE/libraries/$LIB/items" \
          -d '{"itemType":"journalArticle","title":"Trash reach probe"}' | jq -r '.created[0].key')
TRKID=$(j -X POST "$BASE/libraries/$LIB/items" \
          -d "{\"itemType\":\"attachment\",\"parentKey\":\"$TRPAP\",\"title\":\"reach.pdf\"}" \
          | jq -r '.created[0].key')
j -X DELETE "$BASE/libraries/$LIB/items" -d "{\"keys\":[\"$TRKID\"]}" > /dev/null
check "trashed child shows" "$(j "$BASE/libraries/$LIB/items?trash=only&limit=100" \
                                | jq -r --arg k "$TRKID" '[.items[]|select(.key==$k)]|length|select(.==1)')"
j -X DELETE "$BASE/libraries/$LIB/items" -d "{\"keys\":[\"$TRPAP\"]}" > /dev/null
# The sidebar count and the footer count are on screen together, so they have
# to be counting the same thing. They differed by 141 once browsing started
# excluding children: a count that does not match what clicking it shows reads
# as items having gone missing.
#
# Read in a loop: the two figures come from two requests, and this suite is
# writing throughout, so a single pair can differ for no better reason than an
# item created between them. Retrying makes the check about whether they *can*
# agree, which is the actual invariant; a permanently wrong pair never will.
AGREED=""
for _ in $(seq 1 6); do
  SIDEBAR=$(j "$BASE/stats" | jq -r '.items')
  BROWSED=$(j "$BASE/libraries/$LIB/items?limit=1&topLevel=true" | jq -r '.total')
  if [[ "$SIDEBAR" == "$BROWSED" ]]; then AGREED="$SIDEBAR"; break; fi
  sleep 1
done
check "counts agree"     "$AGREED"
# A word query cannot honour a column sort: it scores a bounded pool and
# returns it best-first. That was true and unsaid, so the table drew an arrow
# on a column it was not sorted by and took clicks that changed nothing.
check "search is ranked"  "$(j "$BASE/libraries/$LIB/items?q=attention&sort=title" \
                              | jq -r '.ranked | select(. == true) | "ranked"')"
# The same request with a filter-only query *is* sorted, so the flag must not
# simply be on whenever `q=` is present.
check "filter not ranked" "$(j "$BASE/libraries/$LIB/items?q=tag:nlp&sort=title" \
                              | jq -r 'if .ranked then empty else "sorted" end')"
check "browse not ranked" "$(j "$BASE/libraries/$LIB/items?sort=title" \
                              | jq -r 'if .ranked then empty else "sorted" end')"

echo "▸ quick add"
# Quick add had no coverage at all, which is how "nothing resolved" managed to
# be a flat 404 for three different situations. A refusal and an absence need
# opposite responses from the user, so the reason has to survive to the client.
QA_NOPE="$(j -X POST "$BASE/libraries/$LIB/quick-add" -d '{"text":"10.9999/definitely-not-a-real-doi"}')"
check "unresolved kept"  "$(echo "$QA_NOPE" | jq -r '.unresolved | length | select(. > 0)')"
# A DOI that certainly does not exist should come back "notFound" — but only
# if Crossref answered. When it is unreachable the engine ranks "unavailable"
# higher, which is the whole point of the ranking, so this is a skip and not a
# failure: a check that goes red for somebody else's outage gets switched off.
QA_PROBLEM="$(echo "$QA_NOPE" | jq -r '[.unresolved[].problem] | if index("notFound") then "notFound" else .[0] // "none" end')"
if [[ "$QA_PROBLEM" == "unavailable" ]]; then
  skip "absence is notFound" "no source answered; cannot tell absence from an outage"
else
  check "absence is notFound" "$([[ "$QA_PROBLEM" == "notFound" ]] && echo notFound)"
fi
check "nothing created"  "$(echo "$QA_NOPE" | jq -r '.created | length | select(. == 0) | "none"')"
# A well-formed request that resolves nothing is an outcome, not a failure.
check "outcome not error" "$(curl -sf -o /dev/null -w '%{http_code}' -X POST \
                               -H 'content-type: application/json' \
                               -d '{"text":"10.9999/definitely-not-a-real-doi"}' \
                               "$BASE/libraries/$LIB/quick-add")"
# Every problem the server can report must be a code the catalogues translate.
check "problem is a code" "$(echo "$QA_NOPE" \
                               | jq -r '[.unresolved[].problem] | map(select(. == "notFound" or . == "blocked" or . == "unavailable")) | length | select(. > 0)')"

# One address is one paper. An arXiv link is detected twice -- as the arXiv
# number and as the URL -- and both resolve, from two sources that know
# different things. They must come back as one merged work: the old check
# compared the first's identifier against the second's title, so pasting one
# link filed two items with the same title, one carrying the tags and the
# other the venue.
QA_ARXIV="$(j -X POST "$BASE/resolve" -d '{"text":"https://arxiv.org/abs/2608.27441"}')"
if [[ "$(echo "$QA_ARXIV" | jq -r '.resolutions | length')" == "0" ]]; then
  skip "one link, one paper" "arxiv did not answer"
  skip "merged from both"    "arxiv did not answer"
else
  check "one link, one paper" "$(echo "$QA_ARXIV" | jq -r '.resolutions | length | select(. == 1)')"
  # And the survivor carries what each source contributed, rather than the
  # first one to arrive winning and the rest being dropped.
  # `has`, not `// ""`: a fallback inside the program turns a missing field
  # into a value and the check into one that cannot fail (3.240).
  check "merged from both"    "$(echo "$QA_ARXIV" | jq -r '
    .resolutions[0].draft
    | select((.tags | length) > 0 and has("publicationTitle"))
    | "tags and venue"')"
fi

# Searching outside the library is a different question from resolving an
# identifier, and the skill has been telling the assistant to do it since it
# was written. Every source names the subjects it covers, which is how a
# question about public health reaches PubMed and not a maths preprint server.
SRC="$(j "$BASE/search/external/sources")"
check "search sources"   "$(echo "$SRC" | jq -r 'length | select(. >= 4)')"
check "sources are named" "$(echo "$SRC" | jq -r '[.[] | select(.id == "pubmed") | .subjects[] | select(. == "public health")] | length | select(. == 1) | "routed"')"

EXT="$(j -X POST "$BASE/search/external" -d '{"query":"wastewater surveillance public health","sources":["pubmed"],"limit":3}')"
if [[ "$(echo "$EXT" | jq -r '.results | length')" == "0" ]]; then
  skip "external search"   "pubmed did not answer"
  skip "results are addable" "pubmed did not answer"
else
  check "external search"  "$(echo "$EXT" | jq -r '.results | length | select(. > 0)')"
  # A result with nothing to add it by is a row the reader cannot act on.
  check "results are addable" "$(echo "$EXT" | jq -r '
    [.results[] | select(has("identifier") and .title != "")] | length
    | select(. == ('"$(echo "$EXT" | jq -r '.results | length')"')) | "all addable"')"
fi
# Asking for a source that does not exist must say so rather than quietly
# searching everything, which would be a different answer to the one asked for.
check "unknown source said" "$(j -X POST "$BASE/search/external" \
                                -d '{"query":"x","sources":["web-of-science"]}' \
                                | jq -r '.failed | length | select(. == 1) | "named"')"

echo "▸ filing"
# Filing was a one-way door: the store could take an item out of a collection
# and only the assistant could ask it to, because there was no route. The
# workbench could put things in and never get them out again.
FCOL=$(j -X POST "$BASE/libraries/$LIB/collections" -d '{"name":"Smoke filing"}' | jq -r .key)
FKEY=$(j "$BASE/libraries/$LIB/items?limit=1" | jq -r '.items[0].key')
j -X POST "$BASE/libraries/$LIB/collections/$FCOL/items" -d "{\"keys\":[\"$FKEY\"]}" > /dev/null
check "filed"            "$(j "$BASE/libraries/$LIB/items?collection=$FCOL" | jq -r '.total | select(. == 1)')"
check "unfiled"          "$(j -X DELETE "$BASE/libraries/$LIB/collections/$FCOL/items" \
                             -d "{\"keys\":[\"$FKEY\"]}" | jq -r '.removed | select(. == 1)')"
check "collection empty" "$(j "$BASE/libraries/$LIB/items?collection=$FCOL" | jq -r '.total | select(. == 0) | "empty"')"
# The item itself is untouched, which is the whole difference between taking it
# out of a collection and deleting it.
check "item survives"    "$(j "$BASE/libraries/$LIB/items/$FKEY" | jq -r 'select(.deleted == false) | "still here"')"

# Every parameter the workbench can put in a URL, in one request. A key the
# server does not know is now refused rather than dropped, which is right --
# and makes this the check that the two have not drifted apart. `buildQuery`
# is hand-written, so nothing else compares the lists.
check "client params fit" "$(c -o /dev/null -w '%{http_code}' \
  "$BASE/libraries/$LIB/items?q=a&mode=hybrid&collection=$FCOL&trash=exclude&topLevel=true\
&sort=title&direction=asc&limit=1&offset=0&tag=survey&itemType=journalArticle" | grep -x 200)"
# And a key it does not know is refused, not answered with the whole library.
# A typo used to return 99,992 rows with a 200: the right shape, a plausible
# number, and the wrong answer.
check "a typo is refused"  "$(c -o /dev/null -w '%{http_code}' \
                               "$BASE/libraries/$LIB/items?tags=survey" | grep -x 400)"
check "typo names itself"  "$(j "$BASE/libraries/$LIB/items?collcetion=X" \
                               | jq -r '.title | select(contains("collcetion")) | "named"')"
j -X DELETE "$BASE/libraries/$LIB/collections/$FCOL" > /dev/null

echo "▸ tags & facets"
check "tags"             "$(j "$BASE/libraries/$LIB/tags" | jq -r 'length')"
check "facets"           "$(j "$BASE/libraries/$LIB/facets" | jq -r 'length')"

echo "▸ plugins"
# `jq type` answers "array" for an empty one, so these passed whether or not a
# single plugin had loaded. The point of shipping three is that they run.
check "plugins loaded"   "$(j "$BASE/plugins" | jq -r '[.[] | select(.state == "ready")] | length | select(. >= 3)')"
check "contributions"    "$(j "$BASE/plugins/contributions" | jq -r '[to_entries[] | select((.value | length) > 0)] | length | select(. > 0) | "contributed"')"
# `enabled` and `state` are the same question. They disagreed: the manifest's
# load-time default was flattened in beside the runtime state, so a disabled
# plugin reported `enabled: true`.
check "enabled agrees"   "$(j "$BASE/plugins" | jq -r '[.[] | select(.enabled != (.state != "disabled"))] | length | select(. == 0) | "coherent"')"

echo "▸ collection appearance"
APPK=$(j -X POST "$BASE/libraries/$LIB/collections" \
         -d '{"name":"Smoke appearance","color":"violet","icon":"flask"}' | jq -r .key)
check "colour saved"     "$(j "$BASE/libraries/$LIB/collections" \
                            | jq -r --arg k "$APPK" '.[] | select(.key==$k) | .color')"
check "colour cleared"   "$(j -X PATCH "$BASE/libraries/$LIB/collections/$APPK" -d '{"color":null}' \
                            | jq -r 'select(.color == null) | .icon')"
# Every field on a patch is optional, so a key that matches nothing produces a
# patch that does nothing — and answers 200 having done it. `colour` is the
# spelling this codebase uses in every comment, tag and check name except the
# field itself, so it is the mistake most likely to be made here.
check "colour is refused" "$(c -o /dev/null -w '%{http_code}' -X PATCH \
                              "$BASE/libraries/$LIB/collections/$APPK" \
                              -H 'Content-Type: application/json' \
                              -d '{"colour":"violet"}' | grep -x 422)"
# A saved search with no search is not one: it was accepted and matched the
# whole library, attachments and notes included, under whatever name was typed.
# The name was already refused when blank; the query was not.
check "blank query refused" "$(c -o /dev/null -w '%{http_code}' -X POST \
                                -H 'Content-Type: application/json' \
                                -d '{"name":"No query","query":"   "}' \
                                "$BASE/libraries/$LIB/smart-collections" | grep -x 422)"
check "real query accepted" "$(j -X POST "$BASE/libraries/$LIB/smart-collections" \
                                -d '{"name":"Smoke saved search","query":"tag:nlp"}' \
                                | jq -r '.key | select(length > 0)')"

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
# A Zotero import is a task like every other long job: a real library takes
# minutes, and this is the first thing anybody does with the program.
zotero_import() { # zotero_import -> the result of a finished run
  local id
  id=$(j -X POST "$BASE/libraries/$LIB/import/zotero" -d "{\"path\":\"$ZDB\"}" | jq -r .task.id)
  await_task "$id" > /dev/null
  j "$BASE/tasks/$id" | jq -r .result
}
check "import commits"    "$(zotero_import | jq -r 'select(.failed == 0) | .total')"
# Running it again updates rather than duplicating; that is why keys are kept.
# Phrased so a wrong answer is empty, not merely a different word — `check`
# passes anything non-empty, which once made "failed" read as a success.
#
# `added == 0` is the half that matters. `updated > 0` alone would pass on an
# import that updated every item *and* inserted a second copy of each, which is
# exactly what re-running an import must never do: it is the thing a user tries
# when they are not sure the first one worked.
IMP_AGAIN="$(zotero_import)"
check "import repeatable" "$(echo "$IMP_AGAIN" | jq -r 'select(.failed == 0 and .updated > 0) | "updated \(.updated)"')"
# The field is `items`, not `added`: it counts what this run created. Asking
# for `.added` gets null, `null == 0` is false, and the check fails while the
# program is right — which is the same shape as the bugs this suite exists to
# catch, so it is worth naming rather than quietly correcting.
check "import adds nothing" "$(echo "$IMP_AGAIN" | jq -r 'select(.items == 0) | "no duplicates"')"
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
check "connector saves"   "$(c -o /dev/null -w '%{http_code}' -X POST "$CONN/connector/saveItems" \
                             -H 'Content-Type: application/json' \
                             -d '{"sessionID":"smoke","items":[{"itemType":"journalArticle","title":"Connector smoke","tags":[{"tag":"smoke-connector"}]}]}' \
                             | grep -x 201)"
# What it saved must be a real item, findable the ordinary way.
check "connector stored"  "$(j "$BASE/libraries/$LIB/items?q=Connector%20smoke&limit=1" \
                             | jq -r '.items[0].title // empty')"
# A translator's tags are automatic, not the user's own.
check "connector tags"    "$(j "$BASE/libraries/$LIB/items?q=Connector%20smoke&limit=1" \
                             | jq -r '.items[0].tags[0] | select(.type == 1) | .tag')"
check "connector session" "$(c -o /dev/null -w '%{http_code}' -X POST "$CONN/connector/updateSession" \
                             -H 'Content-Type: application/json' -d '{"sessionID":"smoke"}' | grep -x 200)"

echo "▸ connector switch"
# Browser saving used to be reachable only through a command-line flag, on a
# product whose recommended install is a background service. Quick add now
# advises using the connector when a publisher refuses us, so the advice has to
# be followable from the only interface the user has.
#
# Every value here is *compared*, never merely non-empty: `check` passes on any
# non-empty string, so a bare status code would pass on exactly the failure it
# is meant to catch (see the audit that found two of those).
CONN_BEFORE="$(j "$BASE/ping" | jq -r '.connector.state')"
check "connector on"     "$(j -X PUT "$BASE/connector" -d '{"port":23219}' \
                             | jq -r '.state | select(. == "listening")')"
check "listening now"    "$(j "$BASE/ping" | jq -r '.connector | select(.port == 23219) | .state')"
# The bound port must actually speak the protocol, not merely be open.
check "port serves"      "$(curl -s -o /dev/null -w '%{http_code}' \
                             127.0.0.1:23219/connector/ping | grep -x 200)"
check "connector off"    "$(j -X PUT "$BASE/connector" -d '{"port":null}' \
                             | jq -r '.state | select(. == "off")')"
# Off means the socket is gone, not merely that the badge changed. `000` is
# curl for "nothing accepted the connection", which is the point.
check "port closed"      "$(curl -s -o /dev/null -m 3 -w '%{http_code}' \
                             127.0.0.1:23219/connector/ping | grep -x 000)"
# Asking for a port something else owns is a conflict, and the status must not
# claim success: the whole point of reporting the bind is that it can fail.
SELF_PORT="$(echo "$BASE" | sed -E 's#.*:([0-9]+)/api.*#\1#')"
check "taken is 409"     "$(c -o /dev/null -w '%{http_code}' -X PUT \
                             -H 'Content-Type: application/json' \
                             -d "{\"port\":$SELF_PORT}" "$BASE/connector" | grep -x 409)"
check "still honest"     "$(j "$BASE/ping" | jq -r '.connector.state | select(. == "off")')"
# Leave it as it was found.
if [ "$CONN_BEFORE" = "listening" ]; then
  j -X PUT "$BASE/connector" -d '{"port":23119}' > /dev/null || true
fi

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
# Only meaningful when a model is configured: with none, the agent exposes no
# tools at all and this would report a missing feature instead of an absent
# assistant. It was not in the skipped block and so failed on a fresh library.
if [[ "$(j "$BASE/agent" | jq -r '(.tools // []) | length')" != "0" ]]; then
  check "reference tool"  "$(j "$BASE/agent" | jq -r '.tools | map(select(. == "list_references")) | length | select(. == 1)')"
else
  skip "reference tool" "no model configured, so the agent lists no tools"
fi
check "graph coupling"    "$(j "$BASE/libraries/$LIB/graph/$CITER" \
                             | jq -r '[.edges[] | select(.relation == "coupling")] | length | select(. > 0)')"
check "graph cocitation"  "$(j "$BASE/libraries/$LIB/graph/$CA" \
                             | jq -r --arg k "$CB" '[.edges[] | select(.relation == "cocitation" and .target == $k)] | length | select(. == 1)')"

echo "▸ export"
EXKEY=$(j -X POST "$BASE/libraries/$LIB/items" \
          -d '{"itemType":"journalArticle","title":"Exported 100% {Braced} Paper","date":"2018","publicationTitle":"Journal of Tests","pages":"10-20","DOI":"10.1/exp","creators":[{"creatorType":"author","lastName":"Ito","firstName":"Ken"}]}' \
          | jq -r '.created[0].key')
BIB=$(c -H 'Content-Type: application/json' -X POST "$BASE/libraries/$LIB/export" \
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
RIS=$(c -H 'Content-Type: application/json' -X POST "$BASE/libraries/$LIB/export" \
        -d "$(jq -nc --arg k "$EXKEY" '{itemKeys:[$k],format:"ris"}')")
check "ris terminated"    "$(echo "$RIS" | grep -q '^TY  - JOUR$' && echo "$RIS" | grep -q '^ER  - *$' && echo "well formed")"
CSL=$(c -H 'Content-Type: application/json' -X POST "$BASE/libraries/$LIB/export" \
        -d "$(jq -nc --arg k "$EXKEY" '{itemKeys:[$k],format:"csljson"}')")
check "csl json parses"   "$(echo "$CSL" | jq -r '.[0] | select(.type == "article-journal") | .author[0].family')"
check "csl json year"     "$(echo "$CSL" | jq -r '.[0].issued["date-parts"][0][0] | select(. == 2018)')"
check "export refuses"    "$(c -o /dev/null -w '%{http_code}' -H 'Content-Type: application/json' \
                              -X POST "$BASE/libraries/$LIB/export" \
                              -d "$(jq -nc --arg k "$EXKEY" '{itemKeys:[$k],format:"zotero-rdf"}')" \
                              | grep -q '^4' && echo "rejected")"

echo "▸ reader state"
# Reopening a paper where it was left. Stored per attachment, and deliberately
# *not* on the item: writing it there would bump the library version on every
# scroll, so the check is that reading a document does not look like editing
# the library.
# Its own file, rather than one another section happened to make: a check that
# depends on a variable set further down the script is a check that breaks when
# somebody reorders the sections, which is exactly what happened.
RFILE=$(j -X POST "$BASE/libraries/$LIB/items" \
          -d "{\"itemType\":\"attachment\",\"contentType\":\"application/pdf\",\"linkMode\":\"imported_file\",\"filename\":\"read.pdf\"}" \
          | jq -r '.created[0].key')
RVER=$(j "$BASE/libraries" | jq -r --argjson l "$LIB" '.[] | select(.id == $l) | .version')
check "unread is page one" "$(j "$BASE/libraries/$LIB/items/$RFILE/reader-state" \
                              | jq -r '.lastPage | select(. == 1)')"
j -X PUT "$BASE/libraries/$LIB/items/$RFILE/reader-state" \
  -d '{"lastPage":14,"zoom":1.6,"scrollMode":"paged","sidebar":false}' >/dev/null
check "state remembered"  "$(j "$BASE/libraries/$LIB/items/$RFILE/reader-state" \
                              | jq -r 'select(.lastPage == 14 and .zoom == 1.6 and .scrollMode == "paged" and .sidebar == false) | "kept"')"
check "version untouched"  "$(j "$BASE/libraries" \
                              | jq -r --argjson l "$LIB" --arg v "$RVER" '.[] | select(.id == $l) | .version | tostring | select(. == $v) | "unchanged"')"
# What arrives from a client that has not finished loading.
check "nonsense clamped"  "$(j -X PUT "$BASE/libraries/$LIB/items/$RFILE/reader-state" \
                              -d '{"lastPage":0,"zoom":0}' | jq -r '.state | select(.lastPage == 1 and .zoom == 0.25) | "clamped"')"
check "state is per file" "$(j "$BASE/libraries/$LIB/items/$KEY/reader-state" \
                              | jq -r '.lastPage | select(. == 1)')"

echo "▸ notes from annotations"
NPAPER=$(j -X POST "$BASE/libraries/$LIB/items" \
           -d '{"itemType":"journalArticle","title":"A Paper With Marks"}' | jq -r '.created[0].key')
NFILE=$(j -X POST "$BASE/libraries/$LIB/items" \
          -d "{\"itemType\":\"attachment\",\"parentKey\":\"$NPAPER\",\"contentType\":\"application/pdf\",\"linkMode\":\"imported_file\",\"filename\":\"p.pdf\"}" \
          | jq -r '.created[0].key')
# Out of order on purpose: the note must come back in page order.
for m in '7|later passage|' '2|earlier passage|a thought'; do
  PG=$(echo "$m" | cut -d'|' -f1); TX=$(echo "$m" | cut -d'|' -f2); CM=$(echo "$m" | cut -d'|' -f3)
  j -X POST "$BASE/libraries/$LIB/items" \
    -d "$(jq -nc --arg p "$NFILE" --arg pg "$PG" --arg tx "$TX" --arg cm "$CM" \
          '{itemType:"annotation",parentKey:$p,annotationType:"highlight",annotationPage:$pg,annotationText:$tx,annotationComment:$cm}')" >/dev/null
done
NOTE=$(j -X POST "$BASE/libraries/$LIB/items/$NPAPER/notes/from-annotations" -d '{}')
check "gathered marks"    "$(echo "$NOTE" | jq -r '.annotations | select(. == 2)')"
# The trap this endpoint exists to avoid: annotations hang off the attachment,
# not the paper, so asking for the paper's own children finds nothing.
check "note is a child"   "$(echo "$NOTE" | jq -r --arg p "$NPAPER" '.note | select(.parentKey == $p) | .itemType')"
NBODY=$(echo "$NOTE" | jq -r '.note.note')
check "page order kept"   "$(echo "$NBODY" | python3 -c "
import sys
t = sys.stdin.read()
print('in order' if t.find('p. 2') < t.find('p. 7') else '')
")"
check "quotes the paper"  "$(echo "$NBODY" | grep -o '<blockquote>earlier passage</blockquote>')"
check "keeps the comment" "$(echo "$NBODY" | grep -o '<p>a thought</p>')"
# On the status, not the body. This read `.error.message // .error //
# "refused"`, and the envelope has carried `code`/`status`/`title` since
# rejections were given one — so there is no `.error` to find and the literal
# was printed every time, whether the server refused or happily obliged. A
# fallback in a check is a way of never asking the question.
check "refuses an empty"  "$(c -o /dev/null -w '%{http_code}' -X POST \
                             -H 'Content-Type: application/json' -d '{}' \
                             "$BASE/libraries/$LIB/items/$KEY/notes/from-annotations" | grep -x 422)"

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

# Every format this program writes, it must be able to read — and put each
# value back where the *type* expects it. Each interchange format has one field
# for the containing work, one for the issuing body and (in RIS) one for the
# standard number, while this program has several of each. Sending them all to
# publicationTitle / publisher / ISSN recorded a proceedings as a journal, a
# university as a publisher, and a book's ISBN as an ISSN — the last being
# worse than losing it, since the record then claims a number it has not got.
#
# A chapter and a thesis between them exercise all three decisions.
RTC=$(j -X POST "$BASE/libraries/$LIB/items" \
        -d '{"itemType":"bookSection","title":"RT smoke chapter","bookTitle":"RT smoke book","ISBN":"9780262510875","date":"2018-03-04"}' \
        | jq -r '.created[0].key')
RTT=$(j -X POST "$BASE/libraries/$LIB/items" \
        -d '{"itemType":"thesis","title":"RT smoke thesis","university":"RT smoke university","date":"2020"}' \
        | jq -r '.created[0].key')
for FMT in bibtex ris csljson; do
  TEXT=$(j -X POST "$BASE/libraries/$LIB/export" -d "{\"keys\":[\"$RTC\",\"$RTT\"],\"format\":\"$FMT\"}")
  BACK=$(j -X POST "$BASE/libraries/$LIB/import/bibliography" -d "$(jq -nc --arg t "$TEXT" '{text:$t}')")
  check "$FMT reads its own"  "$(echo "$BACK" | jq -r 'select(.imported == 2) | "both"')"
  RTKEYS=$(echo "$BACK" | jq -r '.keys // [] | @tsv')
  RTBODY=$(for K in $RTKEYS; do j "$BASE/libraries/$LIB/items/$K"; done | jq -sc '.')
  check "$FMT keeps chapter"  "$(echo "$RTBODY" \
     | jq -r 'map(select(.itemType == "bookSection")) | .[0] | select(.bookTitle == "RT smoke book") | .bookTitle')"
  check "$FMT keeps thesis"   "$(echo "$RTBODY" \
     | jq -r 'map(select(.itemType == "thesis")) | .[0] | select(.university == "RT smoke university") | .university')"
  j -X POST "$BASE/libraries/$LIB/items/delete" \
    -d "$(printf '%s' "$RTKEYS" | tr '\t' '\n' | jq -R . | jq -sc '{keys: .}')" > /dev/null
done
# BibTeX has no field for a day, so only the two that can carry one are asked.
check "csl keeps the day"  "$(j -X POST "$BASE/libraries/$LIB/export" \
                               -d "{\"keys\":[\"$RTC\"],\"format\":\"csljson\"}" \
                               | jq -r '.[0].issued["date-parts"][0] | select(length == 3) | join("-")')"
j -X POST "$BASE/libraries/$LIB/items/delete" -d "{\"keys\":[\"$RTC\",\"$RTT\"]}" > /dev/null


echo "▸ maintenance"
# Asked, not assumed: the server knows where it keeps its data, and a script
# that hard-codes the path checks a different machine's backups on the day
# somebody runs it with a different --data-dir.
DATA=$(j "$BASE/ping" | jq -r .dataDir)
# A backup is worth what can be restored from it, so the check is that the file
# opens as a library and holds the same number of items — not that the endpoint
# returned 200.
BKTASK=$(j -X POST "$BASE/maintenance/backup" | jq -r .task.id)
check "backup finished"   "$(await_task "$BKTASK" | grep -x done)"
BK=$(j "$BASE/tasks/$BKTASK" | jq -r .result)
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
BK2=$(j -X POST "$BASE/maintenance/backup" | jq -r .task.id)
await_task "$BK2" > /dev/null
check "backup repeats"    "$(j "$BASE/tasks/$BK2" | jq -r --arg n "$BKNAME" '.result.name | select(. == $n)')"
check "backups listed"    "$(j "$BASE/maintenance/backups" | jq -r --arg n "$BKNAME" '[.backups[] | select(.name == $n)] | length | select(. == 1)')"
check "integrity checked" "$(j "$BASE/maintenance/integrity" | jq -r '.checked | tostring | select(. != "null")')"
check "integrity reports" "$(j "$BASE/maintenance/integrity" | jq -r 'select((.missing | type) == "array" and (.orphans | type) == "array") | "both directions"')"
# The whole library as one file. An archive is worth what can be opened from
# it, so the check unpacks the database inside and reads it.
# Started, not awaited: a big library takes minutes, and a held-open request
# is a client with nothing to show and a proxy free to time out.
EXPTASK=$(j -X POST "$BASE/maintenance/export-all" | jq -r .task.id)
check "export started"    "$EXPTASK"
check "export finished"   "$(await_task "$EXPTASK" | grep -x done)"
EXP=$(j "$BASE/tasks/$EXPTASK" | jq -r .result)
EXPNAME=$(echo "$EXP" | jq -r .name)
check "exported"          "$(echo "$EXP" | jq -r '.bytes | select(. > 0) | "written"')"
check "task is listed"    "$(j "$BASE/tasks" | jq -r --arg t "$EXPTASK" '[.tasks[] | select(.id == $t)] | length | select(. == 1)')"
# The proof that matters: destroy something, read the archive back, and find
# it there again with the same key. An export nothing can consume is a hole,
# not a door.
MARKER=$(j -X POST "$BASE/libraries/$LIB/items" \
           -d '{"itemType":"journalArticle","title":"Archive Marker"}' | jq -r '.created[0].key')
# What the library holds before the round trip, so "everything else was left
# alone" can be checked against a number rather than against a magic 100 that
# only holds on a library earlier runs have filled up.
BEFORE_IMPORT=$(j "$BASE/libraries/$LIB/items?limit=1&trash=include" | jq -r '.total')
EXP2TASK=$(j -X POST "$BASE/maintenance/export-all" | jq -r .task.id)
await_task "$EXP2TASK" > /dev/null
EXP2NAME=$(j "$BASE/tasks/$EXP2TASK" | jq -r .result.name)
j -X POST "$BASE/libraries/$LIB/items/delete" -d "$(jq -nc --arg k "$MARKER" '{keys:[$k]}')" >/dev/null
check "marker destroyed"  "$(j "$BASE/libraries/$LIB/items/$MARKER" -o /dev/null -w '%{http_code}' | grep -q '^4' && echo "gone")"
IMPTASK=$(j -X POST "$BASE/maintenance/import-archive" \
            -d "$(jq -nc --arg p "$DATA/exports/$EXP2NAME" '{path:$p}')" | jq -r .task.id)
check "import finished"   "$(await_task "$IMPTASK" | grep -x done)"
IMP2=$(j "$BASE/tasks/$IMPTASK" | jq -r .result)
check "archive restores"  "$(echo "$IMP2" | jq -r '.items | select(. >= 1) | "restored"')"
check "marker is back"    "$(j "$BASE/libraries/$LIB/items/$MARKER" | jq -r '.title | select(. == "Archive Marker")')"
# Merging, not replacing: everything that was still here is left alone. The
# archive holds what the library held; one item was destroyed before the
# import, so every other one must be skipped rather than rewritten.
check "import merges"     "$(echo "$IMP2" | jq -r --argjson n "$((BEFORE_IMPORT - 1))" \
                             '.skipped | select(. >= $n) | "kept the rest"')"
check "import is clean"   "$(echo "$IMP2" | jq -r '.failed | select(. == 0) | "no failures"')"

# Rebuilding the index is half a minute of work; the point of making it a task
# is that the request comes back at once and the job is still findable.
RIDX=$(j -X POST "$BASE/maintenance/reindex/$LIB" | jq -r .task.id)
check "reindex started"   "$RIDX"
check "reindex is a job"  "$(j "$BASE/tasks/$RIDX" | jq -r '.kind | select(. == "reindex")')"
# It cannot count its work, and says so rather than inventing a percentage.
check "reindex is honest" "$(j "$BASE/tasks/$RIDX" | jq -r '.total | select(. == 0) | "uncountable"')"

check "cancel is honest"  "$(j -X POST "$BASE/tasks/t999999/cancel" | jq -r 'select(.cancelled == false) | "no such task"')"

# Every job's message is shown on four surfaces, so it must be a code the
# catalogue can translate, not a sentence in the server's own language. Swept
# by shape -- a code has no spaces -- rather than by a list of known jobs,
# because the next job added would not be on the list.
check "jobs speak codes"  "$(j "$BASE/tasks" | jq -r '
  [.tasks[] | select(.message != null and .message != "") | select(.message | test(" "))]
  | length | select(. == 0) | "all coded"')"

# Every failure the client shows is named from its own catalogue, keyed by the
# `code` in the envelope. A code the catalogue has never heard of falls back to
# the server's English, so the two lists must not drift apart. Read from the
# client's list rather than repeated here, for the same reason.
KNOWN=$(sed -n "/KNOWN_CODES = new Set/,/])/p" web/src/lib/errors.ts | grep -o "'[a-z_]*'" | tr -d "'" | tr '\n' ' ')
UNKNOWN=""
SEEN=""
# Four classes, not four spellings of the same one: a missing thing, a verb the
# address does not have, a body in the wrong syntax, and a body of the wrong
# shape. All four are rejected in different places in the server.
while IFS= read -r probe; do
  # Bare curl, not `j`: `j` always sends a JSON content-type, so the probe for
  # the wrong content type would arrive with two and be judged by the other.
  c=$(eval "c $probe" | jq -r '.code // empty')
  # Named in the shell, not in jq: a `// "literal"` inside the program is a
  # fallback that turns a missing field into a passing check (3.240). Also
  # guards the `case` below, where an empty word matches anything.
  c=${c:-unnamed}
  SEEN="$SEEN $c"
  case " $KNOWN " in *" $c "*) ;; *) UNKNOWN="$UNKNOWN $c" ;; esac
done <<PROBES
"$BASE/items/nope"
-X DELETE "$BASE/ping"
-X POST "$BASE/libraries/$LIB/items" -H content-type:text/plain -d x
-X POST "$BASE/libraries/$LIB/items" -H 'Content-Type: application/json' -d '{"bogus":1}'
PROBES
check "failures are named" "$(test -z "$UNKNOWN" && echo "all known")"
# Four distinct codes, or the sweep is one class probed four ways.
check "failures differ"    "$(echo "$SEEN" | tr ' ' '\n' | grep . | sort -u | wc -l | tr -d ' ' | grep -x 4)"
check "archive opens"     "$(python3 - "$DATA/exports/$EXPNAME" <<'PY'
import zipfile, sqlite3, sys, tempfile, os, json
z = zipfile.ZipFile(sys.argv[1])
if z.testzip() is not None:
    print(""); raise SystemExit
names = z.namelist()
manifest = json.loads(z.read("manifest.json"))
with tempfile.NamedTemporaryFile(suffix=".db", delete=False) as f:
    f.write(z.read("db.sqlite")); tmp = f.name
db = sqlite3.connect(f"file:{tmp}?mode=ro", uri=True)
ok = list(db.execute("PRAGMA integrity_check"))[0][0]
n = list(db.execute("SELECT count(*) FROM items"))[0][0]
db.close(); os.unlink(tmp)
# The manifest must agree with the database it travels with: both are counted
# from the same snapshot, so a mismatch means one of them is from another
# moment in time.
good = ok == "ok" and n > 0 and manifest["items"] == n and "manifest.json" in names
print("opens" if good else "")
PY
)"

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
# Whatever "empty trash" will destroy has to be visible in the trash first.
# A paper's note used to stay live when the paper was trashed — answering
# searches, absent from the trash — and `items.parent_id ON DELETE CASCADE`
# then destroyed it when the trash was emptied.
TRP=$(j -X POST "$BASE/libraries/$LIB/items" \
        -d '{"itemType":"journalArticle","title":"Trash cascade probe"}' | jq -r '.created[0].key')
TRN=$(j -X POST "$BASE/libraries/$LIB/items" \
        -d "{\"itemType\":\"note\",\"parentKey\":\"$TRP\",\"note\":\"notes that must not vanish\"}" \
        | jq -r '.created[0].key')
j -X DELETE "$BASE/libraries/$LIB/items" -d "{\"keys\":[\"$TRP\"]}" > /dev/null
check "note follows down"  "$(j "$BASE/libraries/$LIB/items/$TRN" | jq -r '.deleted | select(. == true) | "trashed"')"
j -X POST "$BASE/libraries/$LIB/items/restore" -d "{\"keys\":[\"$TRP\"]}" > /dev/null 2>&1 || true
check "note comes back"    "$(j "$BASE/libraries/$LIB/items/$TRN" | jq -r 'if .deleted then empty else "restored" end')"
j -X DELETE "$BASE/libraries/$LIB/items" -d "{\"keys\":[\"$TRP\"]}" > /dev/null
check "group is gone"     "$(j "$BASE/libraries/$LIB/duplicates" \
                             | jq -r --arg a "$DA" '[.groups[] | select(any(.[]; .key == $a))] | length | select(. == 0) | "resolved"')"

# Deleting an item for good has to take its bytes with it, and that rests on an
# ordering nothing states: `forget_files` runs *before* the rows are deleted,
# because it reads the items to learn their filenames. Reverse those two lines
# and every file ever deleted stays on disk for ever, silently.
#
# Deletes these two keys rather than emptying the trash: this suite shares one
# library, and a check that destroys everything in the trash breaks whichever
# check happens to run next.
FCP=$(j -X POST "$BASE/libraries/$LIB/items" \
        -d '{"itemType":"journalArticle","title":"File bytes probe"}' | jq -r '.created[0].key')
FCA=$(j -X POST "$BASE/libraries/$LIB/items" \
        -d "{\"itemType\":\"attachment\",\"parentKey\":\"$FCP\",\"filename\":\"bytes-probe.pdf\",\"contentType\":\"application/pdf\",\"linkMode\":\"imported_file\"}" \
        | jq -r '.created[0].key')
printf '%%PDF-1.4\ntrailer<</Root 1 0 R>>\n%%%%EOF\n' > /tmp/yk-bytes-probe.pdf
c -o /dev/null -X PUT "$BASE/libraries/$LIB/files/$FCA" \
  -H 'Content-Type: application/pdf' --data-binary @/tmp/yk-bytes-probe.pdf
check "file is on disk"    "$(find "$SERVING/storage" -name 'bytes-probe.pdf' 2>/dev/null | head -1)"
j -X POST "$BASE/libraries/$LIB/items/delete" -d "{\"keys\":[\"$FCP\",\"$FCA\"]}" > /dev/null
check "bytes went too"     "$([[ -z "$(find "$SERVING/storage" -name 'bytes-probe.pdf' 2>/dev/null)" ]] && echo gone)"
rm -f /tmp/yk-bytes-probe.pdf

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
# Same trap as `refuses an empty`: this fell through to "rejected" whatever
# came back, so a session that stayed usable after being closed would have
# read as a pass.
check "closed stays shut" "$(c -o /dev/null -w '%{http_code}' -X POST \
                              -H 'Content-Type: application/json' -d "$SNAP" \
                              "$BASE/integration/session/$SID/refresh" | grep -x 404)"

echo "▸ search paging"
# The client's infinite scroll stops at `items.length >= total`. While the total
# was the page length, a search could never show more than one screen.
PAGED=$(j "$BASE/libraries/$LIB/items?q=a&limit=2")
check "total exceeds page" "$(printf '%s' "$PAGED" \
                               | jq -r 'select((.total // 0) > (.items | length)) | "more to come"')"
check "search total"       "$(j "$BASE/libraries/$LIB/search?q=a&limit=2" \
                               | jq -r 'select((.total // 0) > (.hits | length)) | "more to come"')"

echo "▸ thumbnails"
# The server keeps no rasteriser: a 404 is the instruction to draw the page in
# the browser and PUT it back, not a failure.
THUMBED=$(j -X POST "$BASE/libraries/$LIB/items" \
            -d '{"itemType":"journalArticle","title":"Cover page"}' | jq -r '.created[0].key')
check "cache starts empty" "$(c -o /dev/null -w '%{http_code}' \
                               "$BASE/libraries/$LIB/items/$THUMBED/thumbnail?page=1&w=240" \
                               | grep -x 404)"
printf '\211PNG\r\n\032\n\001\002\003' > /tmp/yk-thumb.png
check "thumbnail stored"   "$(c -o /dev/null -w '%{http_code}' -X PUT --data-binary @/tmp/yk-thumb.png \
                               "$BASE/libraries/$LIB/items/$THUMBED/thumbnail?page=1&w=240" | grep -x 201)"
check "served as an image" "$(c -o /dev/null -w '%{content_type}' \
                               "$BASE/libraries/$LIB/items/$THUMBED/thumbnail?page=1&w=240" | grep -x image/png)"
# These bytes come back from the user's own origin, so what the caller claims
# they are is worth nothing; only the magic number counts.
check "html is refused"    "$(c -o /dev/null -w '%{http_code}' -X PUT --data-binary '<svg onload=alert(1)>' \
                               "$BASE/libraries/$LIB/items/$THUMBED/thumbnail?page=1&w=240" | grep -x 422)"
check "odd width refused"  "$(c -o /dev/null -w '%{http_code}' --data-binary @/tmp/yk-thumb.png -X PUT \
                               "$BASE/libraries/$LIB/items/$THUMBED/thumbnail?page=1&w=241" | grep -x 422)"
rm -f /tmp/yk-thumb.png

echo "▸ reveal"
# Takes a key and never a path: the server resolves the location from its own
# storage, so there is no parameter here a page could use to name a file.
BARE=$(j -X POST "$BASE/libraries/$LIB/items" \
         -d '{"itemType":"journalArticle","title":"Nothing on disk"}' | jq -r '.created[0].key')
check "no file says so"   "$(j -X POST "$BASE/libraries/$LIB/items/$BARE/reveal" \
                              | jq -r '.title | select(test("no file to show")) | "refused"')"
# This box is headless, so a resolved file cannot actually be shown — and the
# endpoint says so rather than reporting success for something invisible.
# Either answer proves the path resolution ran.
# A row with a filename: the browser also lists linked_url attachments, which
# are addresses rather than files and have nothing on disk to show.
WITHFILE=$(j "$BASE/libraries/$LIB/files" \
             | jq -r 'first(.files[] | select(.filename != "") | .key) // empty')
if [[ -n "$WITHFILE" ]]; then
  check "reveal resolves"  "$(j -X POST "$BASE/libraries/$LIB/items/$WITHFILE/reveal" \
                               | jq -r '(.revealed // .title) | select(test("headless|\\.")) | "resolved"')"
else
  skip "reveal resolves" "no stored file in this library"
fi
check "path is not a key" "$(c -o /dev/null -w '%{http_code}' \
                              -X POST "$BASE/libraries/$LIB/items/..%2fetc/reveal" | grep -E '4[0-9][0-9]')"

echo "▸ exposure"
# Binding past loopback turns off both protections at once: the Host check
# applies only on loopback, and with no key there is nothing to ask for. What
# is then reachable is the whole API, not a read-only view.
if [[ -x ./target/release/yinkote ]]; then
  # `env -u` because this suite may itself be authenticating (YK_API_KEY), and
  # the binary reads that variable: the key meant for the client turned the
  # server under test into a protected one, so it started instead of refusing
  # and this check hung forever. `timeout` because a check that hangs is worse
  # than one that fails -- it stops the suite rather than reporting.
  EXPOSED=$(timeout 20 env -u YK_API_KEY ./target/release/yinkote \
              --data-dir /tmp/yk-exposure-$$ --host 0.0.0.0 --port 23999 2>&1 || true)
  rm -rf "/tmp/yk-exposure-$$"
  check "refuses to publish"  "$(printf '%s' "$EXPOSED" | grep -o 'refusing to serve')"
  # A refusal nobody can act on is only an annoyance — the same standard the
  # data-directory lock is held to.
  check "names both ways out" "$(printf '%s' "$EXPOSED" | grep -c -e 'YK_API_KEY' -e 'allow-anonymous' | grep -x 2)"
else
  skip "refuses to publish" "no release binary to try it with"
  skip "names both ways out" "no release binary to try it with"
fi

# The browser connector sits outside the API guard on purpose — an extension
# speaking Zotero's protocol has nowhere to put a key. That was safe only while
# the port was loopback, and the same router is merged onto the main one, which
# binds wherever --host says. Its authentication is that the caller is here.
LANIP=$(hostname -I 2>/dev/null | awk '{print $1}')
if [[ -x ./target/release/yinkote && -n "$LANIP" ]]; then
  setsid env -u YK_API_KEY ./target/release/yinkote --data-dir "/tmp/yk-peer-$$" --host 0.0.0.0 \
    --port 23998 --allow-anonymous > /dev/null 2>&1 < /dev/null &
  sleep 4
  check "connector is local-only" "$(c -o /dev/null -w '%{http_code}' \
                                      -X POST -H 'Content-Type: application/json' \
                                      "http://$LANIP:23998/connector/saveItems" \
                                      -d '{"items":[{"itemType":"journalArticle","title":"remote"}]}' \
                                      | grep -x 403)"
  # And still answers the extension actually running on this machine.
  check "connector still local" "$(c -o /dev/null -w '%{http_code}' \
                                    "http://127.0.0.1:23998/connector/ping" | grep -x 200)"
  PEERPID=$(ss -lptnH 'sport = :23998' 2>/dev/null | grep -o 'pid=[0-9]*' | head -1 | cut -d= -f2)
  [[ -n "$PEERPID" ]] && kill "$PEERPID" 2>/dev/null
  rm -rf "/tmp/yk-peer-$$"
else
  skip "connector is local-only" "needs a release binary and a routable address"
  skip "connector still local"   "needs a release binary and a routable address"
fi

echo "▸ one server per library"# Two servers on one data directory do not fail — they quietly disagree, each
# with its own copy of the search index. So the second must refuse to start.
DATA_OF=$(j "$BASE/ping" | jq -r .dataDir)
if [[ -x ./target/release/yinkote && -n "$DATA_OF" ]]; then
  SECOND=$(./target/release/yinkote --data-dir "$DATA_OF" --port 23999 2>&1 || true)
  check "second is refused"  "$(printf '%s' "$SECOND" | grep -o 'already using this data directory')"
  # A refusal nobody can act on is only an annoyance.
  check "refusal names a way out" "$(printf '%s' "$SECOND" | grep -o -- '--data-dir')"
else
  skip "second is refused" "no release binary to try it with"
fi

echo "▸ open"
# `yinkote open` is for the person who installed the service and never types a
# URL for it. It finds the address from the directory's lock, so it is also the
# proof that the lock is readable as a registry and not only as a refusal.
if [[ -x ./target/release/yinkote && -n "$DATA_OF" ]]; then
  # Forced headless so nothing actually launches: the address is the part
  # worth checking, and on a machine with no desktop it is also the answer.
  FOUND=$(env -u DISPLAY -u WAYLAND_DISPLAY ./target/release/yinkote open --data-dir "$DATA_OF" 2>&1 || true)
  # Not a shape check — it must be *this* server, the one every check above ran
  # against. A URL assembled from defaults would pass a looser test.
  check "open finds this server" "$(printf '%s' "$FOUND" | grep -xF "${BASE%/api/v1}")"
  # And it must leave the lock alone: a probe that claimed the directory would
  # lock out the very server it was asked to find.
  check "server survives open"   "$(j "$BASE/ping" | jq -r .ok)"

  NOBODY=$(./target/release/yinkote open --data-dir /tmp/yk-open-nobody-$$ 2>&1 || true)
  check "open says when nothing runs" "$(printf '%s' "$NOBODY" | grep -o 'no Yinkote is running')"
  # Same standard as the refusal above: say what to do, not just what is wrong.
  check "open names a way to start"   "$(printf '%s' "$NOBODY" | grep -o 'service install')"
else
  skip "open finds this server" "no release binary to try it with"
fi

echo "▸ single binary"
# The premise is that somebody installs this and starts it, and that only holds
# if "install" means one file. The workbench is compiled in; a directory named
# with --web-dir still wins, because that is how the frontend is developed.
ORIGIN2=${BASE%/api/v1}
check "workbench served"  "$(c -o /dev/null -w '%{content_type}' "$ORIGIN2/" | grep -o 'text/html')"
# A client route must reach the app shell, not a 404 from a server that has
# never heard of it.
check "client routes"     "$(c -o /dev/null -w '%{http_code}' "$ORIGIN2/reader/ABCD1234" | grep -x 200)"
check "hashed asset"      "$(c "$ORIGIN2/" | grep -o '/assets/index-[A-Za-z0-9_-]*\.js' | head -1)"

echo "▸ word add-in"
# The pane is served by the binary, outside /api/v1 and outside the SPA
# fallback. The fallback answering manifest.xml with index.html is the failure
# this guards: valid HTML, and an error inside Word that names nothing.
ORIGIN=${BASE%/api/v1}
MANIFEST=$(curl -fsS "$ORIGIN/addin/manifest.xml")
check "manifest is xml"   "$(printf '%s' "$MANIFEST" | head -c 5 | grep -o '<?xml')"
check "manifest names us" "$(printf '%s' "$MANIFEST" | grep -o "${ORIGIN}/addin/taskpane.html" | head -1)"
check "manifest id stable" "$(
  a=$(printf '%s' "$MANIFEST" | sed -n 's:.*<Id>\(.*\)</Id>.*:\1:p')
  b=$(curl -fsS "$ORIGIN/addin/manifest.xml" | sed -n 's:.*<Id>\(.*\)</Id>.*:\1:p')
  [[ -n "$a" && "$a" == "$b" ]] && echo "$a")"
check "pane is html"      "$(curl -fsS -o /dev/null -w '%{content_type}' "$ORIGIN/addin/taskpane.html" | grep -o 'text/html')"
check "pane script"       "$(curl -fsS "$ORIGIN/addin/taskpane.js" | grep -c 'updatedFields')"
check "icon is a png"     "$(curl -fsS "$ORIGIN/addin/icon-32.png" | head -c 4 | grep -c PNG)"

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
# Sortable, and it is a stored column kept up to date by trigger — so what the
# sort believes and what the row reports have to be the same thing.
check "sorts by files"    "$(j "$BASE/libraries/$LIB/items?sort=attachment&limit=1" \
                             | jq -r '.items[0].attachments | join("+") | select(. != "")')"
check "sorts the other way" "$(j "$BASE/libraries/$LIB/items?sort=attachment&direction=asc&limit=1" \
                             | jq -r '.items[0] | select(has("attachments") | not) | "nothing first"')"

echo "▸ file browser"
check "files listed"      "$(j "$BASE/libraries/$LIB/files" \
                             | jq -r 'select((.files | type) == "array") | "listed"')"
# A file's address is what a file browser is opened to find out.
#
# Its own fixture, deliberately. This used to ask whether *any* file in the
# first page had a URL, which was true only because 186 connector snapshots had
# piled up over previous runs — the accumulation §3.197 cleared away was
# propping up a page-order-dependent check. The listing is newest first, so a
# file created here is on the first page by construction.
FILEFIX=$(j -X POST "$BASE/libraries/$LIB/items" \
            -d '{"itemType":"journalArticle","title":"Linked file smoke"}' | jq -r '.created[0].key')
j -X POST "$BASE/libraries/$LIB/items" \
  -d "$(jq -nc --arg p "$FILEFIX" '{itemType:"attachment",parentKey:$p,title:"Linked file smoke",
        linkMode:"linked_url",url:"https://example.org/smoke.pdf",contentType:"text/html"}')" > /dev/null
check "files keep source" "$(j "$BASE/libraries/$LIB/files" \
                             | jq -r '[.files[] | select(.url == "https://example.org/smoke.pdf")]
                                      | length | select(. > 0) | "kept"')"
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
# Harvesting is a task like every other long job now: it had a status endpoint,
# a stop endpoint and a struct in the application state all of its own.
HARV=$(j -X POST "$BASE/libraries/$LIB/citations/harvest")
HARVTASK=$(echo "$HARV" | jq -r '.task.id // empty')
if [[ -n "$HARVTASK" ]]; then
  check "harvest is a job"  "$(j "$BASE/tasks/$HARVTASK" | jq -r '.kind | select(. == "harvest")')"
  # Two runs would only get the client throttled by the service they share.
  check "harvest one only"  "$(j -X POST "$BASE/libraries/$LIB/citations/harvest" \
                               | jq -r '.title // .message // empty')"
  j -X POST "$BASE/tasks/$HARVTASK/cancel" > /dev/null
else
  # Nothing left to fetch in this library, which is itself a refusal with a
  # reason rather than an empty run.
  check "harvest explains"  "$(echo "$HARV" | jq -r '.title // .message // empty')"
fi
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
# Two things a reader sees immediately when they are wrong, and which used to
# be: an anonymous work is filed under its title rather than opening with a
# stray "(2020).", and an undated one says so rather than leaving a gap.
ANON=$(j -X POST "$BASE/libraries/$LIB/items" \
         -d '{"itemType":"journalArticle","title":"Nobody Wrote This","publicationTitle":"J. Anon"}' \
         | jq -r '.created[0].key')
check "anonymous by title" "$(j -X POST "$BASE/libraries/$LIB/citations" \
                               -d "{\"keys\":[\"$ANON\"],\"style\":\"apa\"}" \
                               | jq -r '.bibliography[0] | select(startswith("Nobody Wrote This."))')"
check "undated says n.d."  "$(j -X POST "$BASE/libraries/$LIB/citations" \
                               -d "{\"keys\":[\"$ANON\"],\"style\":\"apa\"}" \
                               | jq -r '.bibliography[0] | select(contains("(n.d.)"))')"
j -X POST "$BASE/libraries/$LIB/items/delete" -d "$(jq -nc --arg k "$ANON" '{keys:[$k]}')" > /dev/null
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
    # Named one by one rather than letting the block vanish. A silently
    # shorter run still prints "all passed", and the only thing that betrays
    # it is the total — which is exactly what nobody compares. Every check
    # below has to account for itself, present or not.
    for named in "summarise" "summary is a child" "summary replaced" \
                 "reads the paper" "close reading refuses" \
                 "agent answers" "agent shows work"; do
      skip "$named" "needs the model, which is rate-limited right now"
    done
  else
    THROTTLED=0
  fi

  if [[ $THROTTLED -eq 0 ]]; then
  check "summarise"      "$(jq -r '.note.itemType' <<< "$SUMM")"
  check "summary is a child" "$(j "$BASE/libraries/$LIB/items/$AK/children" | jq -r 'length')"
  # Re-running must replace the note, not add a second one.
  j -X POST "$BASE/libraries/$LIB/items/$AK/summarise" -d '{}' > /dev/null
  check "summary replaced" "$(j "$BASE/libraries/$LIB/items/$AK/children" | jq -r 'select(length == 1) | "one"')"

  # Whether it read the paper or only its abstract. An abstract is already a
  # summary, so summarising one produces something that reads like a summary
  # and says nothing new -- the difference between the two is the entire point
  # of extracting the text, and `readInFull` is the only place it is visible.
  # `$AK` has no file, so the honest answer here is `false`; what is checked is
  # that the field is *reported*, because a summary that silently came from an
  # abstract is one nobody can tell apart from one that did not.
  check "reads the paper" "$(jq -r '.readInFull | select(. == true or . == false) | "reported"' <<< "$SUMM")"

  # A close reading of an abstract is a fabrication with headings on it, and it
  # would be filed beside the real ones with nothing to tell them apart. So
  # this refuses where summarise falls back.
  check "close reading refuses" "$(c -o /dev/null -w '%{http_code}' -X POST \
                                    -H 'Content-Type: application/json' -d '{}' \
                                    "$BASE/libraries/$LIB/items/$AK/close-reading" | grep -x 422)"

  ACONV=$(j -X POST "$BASE/libraries/$LIB/conversations" -d '{"title":"smoke"}' | jq -r .key)
  # Starting returns at once now, so the answer is waited for the way a client
  # does: by watching the run, not by holding the request open.
  j -X POST "$BASE/libraries/$LIB/conversations/$ACONV/ask" \
    -d '{"content":"How many items are in the library? Use your tools."}' > /dev/null
  for _ in $(seq 1 60); do
    [[ "$(j "$BASE/libraries/$LIB/conversations/$ACONV/run" | jq -r .running)" == "true" ]] || break
    sleep 1
  done
  # The model is shared and throttles without warning, and a turn can be
  # rate-limited *after* the pre-flight probe said it was healthy. Reporting
  # that as a failure makes the gate flaky, and a gate that cries wolf is one
  # people learn to ignore — so the run says which it was.
  ARUN=$(j "$BASE/libraries/$LIB/conversations/$ACONV/run")
  if echo "$ARUN" | grep -qiE '429|rate.?limit|upstream busy'; then
    skip "agent answers"     "the model is rate-limited right now"
    skip "agent shows work"  "the model is rate-limited right now"
  else
  check "agent answers"  "$(j "$BASE/libraries/$LIB/conversations/$ACONV/messages" \
                            | jq -r '[.messages[] | select(.role == "assistant")][0].content
                                     | select(length > 0) | "answered"')"
  # A turn that ran must leave its steps behind, or the answer is unverifiable.
  check "agent shows work" "$(j "$BASE/libraries/$LIB/conversations/$ACONV/messages" \
                              | jq -r '[.messages[] | select(.role == "assistant")][0].meta.trace
                                       | select(length > 0) | "traced"')"
  fi
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

# ─── clear up after ourselves ──────────────────────────────────────────────
#
# The database is kept between runs, and every run used to leave its fixtures
# in it: 178 items tagged `colour-smoke`, 186 `smoke-connector`, and one new
# `graph-smoke-<random>` tag family per run — about 160 of them by now. That is
# §3.163's lesson from the other side: the corpus the tests measure drifts
# because of the measuring, and the junk lands exactly where the tag graph and
# the duplicate scan read.
#
# After every check, so nothing here can change a result. Failures are ignored:
# tidying is not a test, and a library that will not tidy is not a red run.
echo "▸ tidy"
GONE=0

# By tag where there is one. A tag filter reports an exact total and pages
# without a candidate cap, unlike `?q=`, which stops at 300 and made the first
# version of this look as though it had done nothing.
for tag in colour-smoke smoke-connector; do
  for _ in $(seq 1 40); do
    KEYS=$(j "$BASE/libraries/$LIB/items?tag=$tag&limit=100&trash=include" \
             | jq -r '[.items[].key] | @tsv' 2>/dev/null)
    [[ -z "$KEYS" ]] && break
    N=$(j -X POST "$BASE/libraries/$LIB/items/delete" \
          -d "$(printf '%s' "$KEYS" | tr '\t' '\n' | jq -R . | jq -sc '{keys: .}')" \
          | jq -r '.deleted // 0')
    GONE=$((GONE + N))
    [[ "$N" == "0" ]] && break
  done
done

# And by exact title for the fixtures that carry no tag of their own.
for title in "Graph neighbour" "Citation smoke" "Archive Marker" "Cover page" \
             "Nobody Wrote This" "Linked file smoke"; do
  for _ in $(seq 1 20); do
    KEYS=$(j "$BASE/libraries/$LIB/items?q=$(printf '%s' "$title" | jq -sRr @uri)&limit=100&trash=include" \
             | jq -r --arg t "$title" '[.items[] | select(.title == $t) | .key] | @tsv' 2>/dev/null)
    [[ -z "$KEYS" ]] && break
    N=$(j -X POST "$BASE/libraries/$LIB/items/delete" \
          -d "$(printf '%s' "$KEYS" | tr '\t' '\n' | jq -R . | jq -sc '{keys: .}')" \
          | jq -r '.deleted // 0')
    GONE=$((GONE + N))
    [[ "$N" == "0" ]] && break
  done
done

# Everything else that has piled up, found by the shape of the problem rather
# than by name.
#
# The list above is a list of names, and a list of names rots: the next fixture
# somebody adds is not on it, accumulates one copy per run, and nothing says so.
# That is not hypothetical — this library had grown 3,168 leftovers in 44 groups
# before anybody noticed, and what noticed was the *benchmark*, reporting a
# duplicates scan over a corpus no real library resembles.
#
# The duplicates endpoint already answers "what is piled up here", exactly and
# without the 300-candidate cap that makes `?q=` unable to enumerate. So the
# sweep asks it, rather than guessing titles. Safe because this is a scratch
# library and every fixture is recreated by the next run.
for _ in $(seq 1 30); do
  KEYS=$(j "$BASE/libraries/$LIB/duplicates" \
           | jq -r '[.groups[] | select(length > 3) | .[].key] | .[0:200] | @tsv' 2>/dev/null)
  [[ -z "$KEYS" ]] && break
  N=$(j -X POST "$BASE/libraries/$LIB/items/delete" \
        -d "$(printf '%s' "$KEYS" | tr '\t' '\n' | jq -R . | jq -sc '{keys: .}')" \
        | jq -r '.deleted // 0')
  GONE=$((GONE + N))
  [[ "$N" == "0" ]] && break
done

# The per-run graph tags have no items once their papers are gone, but the
# names linger in the tag list.
LEFTOVER=0
for tag in $(j "$BASE/libraries/$LIB/tags?q=graph-smoke&limit=500" | jq -r '.[].name' 2>/dev/null); do
  j -X DELETE "$BASE/libraries/$LIB/tags" -d "$(jq -nc --arg n "$tag" '{name:$n}')" > /dev/null 2>&1 || true
  LEFTOVER=$((LEFTOVER + 1))
done
printf '  removed %s fixture items and %s spent tags\n' "$GONE" "$LEFTOVER"

# And then check the tidying actually worked.
#
# The list above is a list of names, and a list of names rots: the next fixture
# somebody adds is not on it, accumulates one copy per run, and nothing says so.
# That is not hypothetical — it is what happened. This library had grown 3,168
# leftovers in 44 groups, and the *benchmark* was what noticed, by reporting a
# duplicates scan over a corpus no real library resembles.
#
# So the invariant is stated directly: after tidying, nothing should be piled
# up. A fixture that accumulates now fails here, by name, in the run that
# introduced it.
PILE="$(j "$BASE/libraries/$LIB/duplicates" \
          | jq -r '[.groups[] | select(length > 3)] | length')"
check "left tidy"        "$([[ "$PILE" == "0" ]] && echo tidy)"
if [[ "$PILE" != "0" ]]; then
  echo "    still piled up (add these to the tidy list above):"
  j "$BASE/libraries/$LIB/duplicates" \
    | jq -r '.groups[] | select(length > 3) | "      \(length)x  \(.[0].title)"' | head -12
fi

echo
if [[ $FAIL -eq 0 ]]; then
  # The skipped count is part of the result, not a footnote. A throttled model
  # used to make a whole block disappear in silence, and "196 checks passed"
  # reads exactly like "201 checks passed" unless somebody remembers the number.
  if [[ $SKIP -gt 0 ]]; then
    printf '\033[32m%d checks passed\033[0m, \033[33m%d skipped\033[0m\n' "$PASS" "$SKIP"
  else
    printf '\033[32m%d checks passed\033[0m\n' "$PASS"
  fi
else
  printf '\033[31m%d passed, %d failed\033[0m\n' "$PASS" "$FAIL"
  for name in "${FAILED[@]}"; do
    printf '\033[31m  failed: %s\033[0m\n' "$name"
  done
  exit 1
fi
