# Journal Metrics

Adds three columns to the item table: **IF**, **JCR** and **CAS**.

## Where the numbers come from

The plugin asks three places, in order, and stops at the first answer:

1. **`metrics.json`** — a table you control, keyed by ISSN or by normalised
   journal name. This is where a licensed dataset goes.
2. **`cache.json`** — what OpenAlex has already been asked. Written
   automatically; delete it to force a refresh.
3. **OpenAlex**, live, batched by ISSN and cached.

## What the IF column actually shows

OpenAlex publishes a journal's **two-year mean citedness**. That is the same
shape of measure as Clarivate's Journal Impact Factor, computed over a
different corpus, and it is **not** the JIF. The tooltip says which one you are
looking at, and values from OpenAlex say so explicitly.

If you have a JIF licence, put the real numbers in `metrics.json` and they take
precedence — no value from OpenAlex will override them.

## JCR quartiles and CAS tiers

Both rankings are proprietary. They cannot be derived from open data and are
not distributed here, so those two columns stay empty until you supply them.

`metrics.json` entries look like this — any subset of the three is fine:

```json
{
  "0028-0836":  { "if": 50.5, "jcr": "Q1", "cas": 1 },
  "1538-3598":  { "jcr": "Q1", "cas": 1 },
  "natureaging": { "cas": 2 }
}
```

Keys are either an ISSN (`0028-0836`, exactly as printed) or a journal name
reduced to lowercase letters and digits (`Nature Aging` → `natureaging`). List
both the print and electronic ISSN if you have them: records in the wild carry
either.

Reload without restarting the server:

```
POST /api/v1/plugins/reload
```
