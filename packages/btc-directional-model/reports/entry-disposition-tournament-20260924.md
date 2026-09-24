# Entry and disposition tournament, September 24, 2026

The run is archived at `/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/manifests/run.json`. Its ledgers, qualification checks, source coverage, and chronological activity charts remain with that SSD run. The training branch is `training/entry-disposition-tournament`.

The tournament used the frozen conservative selective live pilot as Entry 1. Entry 2 trained a new directional model and a separate trade admission model, then bought and held. Entry 3 used exactly the Entry 2 buys with a separately trained sell model. Entry 4 let the frozen champion buy first at each checkpoint and allowed the Entry 2 specialist only while the champion had abstained. All entries used five shares in the primary comparison and full-order FOK replay from causal archived ask and bid ladders. The buy and sell ledgers have identical market, side, time, cost, and quantity for all 100 buys.

| Entry | Buys | Win rate | Wilson lower | Stressed PnL | Stressed profit factor | Buys per fully covered active day |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Frozen champion | 193 | 51.3% | 44.3% | $19.68 | 1.09 | 2.90 |
| New buy and hold | 100 | 53.0% | 43.3% | -$13.10 | 0.90 | 2.86 |
| Same buys, learned sells | 100 | 53.0% | 43.3% | -$13.28 | 0.90 | 2.86 |
| Champion plus specialist | 234 | 51.7% | 45.3% | $3.96 | 1.01 | 3.56 |

**Decision: no candidate qualifies.** All challengers missed the agreed 78% win rate, 68% Wilson lower bound, 1.5 stressed profit factor, and 5–8 buys per day objective. The buy model reached only seven represented trading weeks, with 100 fills. The sell model completed one sale and reduced matched stressed PnL by $0.18; a fixed exit control did better than the learned sell model, but it also did not qualify. The champion plus specialist gained 41 buys but displaced 21 later champion trades and produced lower stressed PnL than the frozen champion alone. The challenger direction and admission prediction ECE values were 0.0062 and 0.0047; calibration did not translate into profitable entry selection.

Source coverage spans July 13 through September 23. All 73 calendar days have some archived books, but only 41 meet at least 280 complete markets and 95% paired book checkpoint coverage. The charts mark partial coverage and policy-disabled weeks separately from true no-trade days. The copy-only book drain completed through September 23 without deleting source rows. The rebuilt 30–215 second feature panel matched the existing overlapping panel exactly; the frozen champion scorer matched all 256 runtime reference probabilities.

Quantity sensitivity held each Q5 entry decision time and side fixed, then replayed all 14 quantities with full-order FOK fills. Q5 primary replay matched the original 100 buys exactly. Under the primary setting, Q5 hold PnL was -$13.10; at 10, 25, 50, 100, and 200 shares, completed fills fell to 85, 64, 46, 30, and 18. The adverse 300 ms / 65% depth setting reduced Q5 fills to 97 and stressed hold PnL to -$19.62. This is frozen-decision capacity evidence, not quantity-specific policy selection.

The champion's historical artifact was trained on dates overlapping this replay, so its historical score is descriptive. A prospective paper comparison would be required before any deployment decision. No process configuration, runtime model, database schema, image, or deployment changed.
