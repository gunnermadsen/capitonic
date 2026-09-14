# Tournament Coverage and Independence Audit

## Conclusion

Coverage did not mechanically shrink because this was a subsequent tournament. The first time-bucket tournament and this micro-bucket tournament use the same immutable full-history training and evaluation panels, with matching SHA-256 identities and the same March 21 through August 14 fit boundary.

There are two narrower evidence effects:

1. August 14 is deliberately quarantined as a settlement-source transition day. This removes 288 markets from policy selection compared with the prior tournament, preventing refprice/TWAP cutover contamination.
2. The derived early causal feature panel has materially shorter history and only one day of confirmation coverage. This is a source-family limitation, not cumulative tournament attrition.

## Measured coverage

| Evidence window | Full panel markets / days | Early causal markets / days | Early retention |
|---|---:|---:|---:|
| Fit: Mar 21–Aug 14 | 42,046 / 146 | 18,133 / 67 | 43.13% |
| Transition quarantine: Aug 14–15 | 288 / 1 | intentionally excluded | — |
| Policy: Aug 15–20 | 1,437 / 5 | 1,425 / 5 | 99.16% |
| Design: Aug 20–26 | 1,728 / 6 | 1,716 / 6 | 99.31% |
| Confirmation: Aug 26–Sep 1 | 1,662 / 6 | 239 / 1 | 14.38% |

The previous tournament's policy window included August 14 and contained 1,725 markets over six days. The new clean policy window retains 1,437 markets, or 83.30%, while preserving all five uncontaminated post-cutover policy days.

## Effect on the result

- The 150–169 and 185–209 second specialists use the full six-day confirmation panel. Their result is not weakened by the early causal panel's shorter tail.
- The 40–44, 60–64 and 65–69 second causal specialists have only August 26 confirmation evidence. Their apparent confirmation behavior is much less certain and must not be described as a six-day result.
- The selected router's $676.38 result combines early layers with one-day confirmation and later layers with six-day confirmation. It is valid as the exact replay executed, but it is not uniform six-day confirmation for every constituent layer.
- The one-day all-router common comparison covers August 26 through August 27 exclusive and 239 markets. It is directionally useful, not strong repeatability evidence.

## Cross-tournament independence

Within this run, model fitting, policy selection, design validation and confirmation are chronologically disjoint. Across the research program, however, the August periods are no longer globally blind: historic tournament outcomes informed the candidate roster and prior weights, and the immediately preceding tournament used the same configured design and confirmation boundaries.

This does not reduce row coverage, but it does reduce statistical independence. Repeatedly choosing buckets, model families and policies after observing the same dates can create meta-overfitting and make subsequent tournament improvements look more certain than they are.

Accordingly, the correct interpretation is:

- the RTDS-free router and especially the 185–209 DOWN specialist show strong historical and chronological evidence;
- the new router legitimately beat the prior individual champion on the available replay;
- the result is not equivalent to a fresh, never-observed forward test;
- early-bucket repeatability remains unconfirmed beyond one day in the current causal feature panel.

No database, source, schema, ingester, runtime model or trading process was changed for this audit.
