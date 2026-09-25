# Entry and disposition tournament: trade-level diagnosis

**Evaluation:** July 13–September 23, 2026 (73 UTC days). Five-share entries. Causal archived order books; full-order FOK replay at 150 ms latency and 80% displayed-depth limit. No candidate qualified or was deployed. The [run manifest](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/manifests/run.json), [deep-dive metrics](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/metrics/deep-dive.json), [daily economics](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/metrics/daily-economics.parquet), and [time slices](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/metrics/time-slices.parquet) preserve the detailed evidence.

## Executive diagnosis

The new buy policy is **not uniformly weak**. Its 17 fills placed 120–149 seconds after market start earned **+$11.72 stressed**, while its 26 fills placed 150–179 seconds after start lost **−$30.15 stressed**. The latter bucket lost in July, August, and September. This is a specific entry/price-selection problem to investigate, but the split was discovered after seeing the evaluation data and cannot be used as a validated trading rule.

Exit choice also matters. Buy-and-hold and learned-sell used exactly the same 100 entry markets, sides, times, prices, and quantities. The learned sell generated one sell intent and completed that one sale, changing stressed PnL by **−$0.18**. A fixed-exit control on those same buys sold at the first eligible checkpoint when the observed bid differed from entry cost by at least $0.10 in either direction (at least five seconds after entry, before second 220, subject to the same FOK replay). It finished at **+$1.74 stressed**, compared with **−$13.10** for holding. Its stressed profit factor was only 1.03; it is a useful control, not a qualified candidate.

The champion-plus-specialist composition made more trades but diluted the champion. Its 172 retained champion trades earned **+$28.06 stressed**; 62 specialist trades lost **−$24.10**. Versus the champion alone, 41 specialist-only markets lost **−$11.88** and 21 displaced later champion trades cost another **−$3.85**. These reconcile to the composition's **−$15.73** difference from the champion.

The frozen champion remains the best primary ledger at **+$19.68 stressed**, but September alone lost **−$20.75** and its stressed trade drawdown reached **$50.44**. Its historical artifact was trained on overlapping dates, so the replay is a descriptive baseline rather than independent deployment proof.

## PnL definitions and complete economics

`Net` includes replay execution and reserve. `Stressed` subtracts another $0.05 for each five-share buy and another $0.05 for each completed sale. Gain/loss below sums **positive/negative trade PnL after reserve**, rather than exchange turnover. Profit factor (PF) divides aggregate positive PnL by absolute negative PnL. A winning trade has positive realized net PnL. For a held binary contract this equals correct final direction; a profitable early sale can differ. Recovery is average stressed loss divided by average stressed win: average wins needed to offset one average loss. Drawdown uses trades in chronological order, beginning at zero. All dollar values are for the tested five-share sizing, not annualized returns.

| Ledger | Fills | Positive / negative | Correct final direction | Net gain / loss | Net PnL | Net PF | Stressed gain / loss | Stressed PnL | Stressed PF |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Frozen champion | 193 | 99 / 94 | 51.3% | $254.47 / $225.14 | +$29.33 | 1.13 | $249.52 / $229.84 | **+$19.68** | **1.09** |
| New buy and hold | 100 | 53 / 47 | 53.0% | $119.68 / $127.78 | −$8.10 | 0.94 | $117.03 / $130.13 | **−$13.10** | **0.90** |
| Same buys, learned sell | 100 | 53 / 47 | 53.0% | $119.55 / $127.78 | −$8.23 | 0.94 | $116.85 / $130.13 | **−$13.28** | **0.90** |
| Champion plus specialist | 234 | 121 / 113 | 51.7% | $297.77 / $282.12 | +$15.66 | 1.06 | $291.72 / $287.77 | **+$3.96** | **1.01** |
| Same buys, fixed-exit control | 100 | 58 / 42 | 53.0% | $65.94 / $55.35 | +$10.59 | 1.19 | $60.79 / $59.05 | **+$1.74** | **1.03** |

| Ledger | Stressed PnL / fill | Mean stressed win / loss | Wins to recover loss | Largest stressed win / loss | Stressed trade drawdown | Positive / negative trading days | Buys / fully covered active day |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Frozen champion | +$0.102 | $2.52 / $2.45 | 0.97 | +$3.47 / −$2.83 | $50.44 | 33 / 28 | 2.90 |
| New buy and hold | −$0.131 | $2.21 / $2.77 | 1.25 | +$2.33 / −$3.33 | $24.53 | 14 / 19 | 2.86 |
| Same buys, learned sell | −$0.133 | $2.20 / $2.77 | 1.26 | +$2.33 / −$3.33 | $24.53 | 14 / 19 | 2.86 |
| Champion plus specialist | +$0.017 | $2.41 / $2.55 | 1.06 | +$3.47 / −$3.33 | $57.52 | 37 / 27 | 3.56 |
| Same buys, fixed-exit control | +$0.017 | $1.05 / $1.41 | 1.34 | +$2.83 / −$3.33 | $13.38 | 21 / 12 | 2.86 |

Five-share entry notional by table order was **$460.84 / $270.60 / $270.60 / $583.49 / $270.60**. Reserve deductions were **$4.83 / $2.50 / $2.53 / $5.85 / $4.43**; additional stress deductions were **$9.65 / $5.00 / $5.05 / $11.70 / $8.85**. The fixed-exit control completed 77 sales; learned sell completed one. The [deep-dive metrics](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/metrics/deep-dive.json) also give median PnL, unstressed expectancy, net drawdown, average share cost, full day metrics, and each slice's own gain/loss and PF.

The champion's edge was economic rather than a high win percentage. Its average stressed win ($2.52) slightly exceeded its average loss ($2.45), helped by a lower average entry share cost (about $0.48 versus $0.54 for the new buys). The new hold model won 53.0% of directions, but its wins averaged $2.21 and losses $2.77. More correct directions did not offset weaker payout economics.

## Entry age within the five-minute market

The bucket is **seconds elapsed from market start at the entry checkpoint**. Cells give filled trades / total stressed PnL; each column reconciles to its ledger total. The 210–239 bucket contains entries only through the configured 215-second cutoff.

| Entry age | Champion | New hold | Learned sell | Champion + specialist | Fixed exit |
| --- | ---: | ---: | ---: | ---: | ---: |
| 30–59 sec | 8 / −$5.05 | 0 | 0 | 8 / −$5.05 | 0 |
| 60–89 sec | 20 / +$0.65 | 15 / +$8.12 | 15 / +$8.12 | 29 / +$4.12 | 15 / +$2.20 |
| 90–119 sec | 18 / +$3.76 | 7 / −$4.18 | 7 / −$4.18 | 22 / −$7.34 | 7 / +$3.90 |
| 120–149 sec | 27 / +$3.57 | **17 / +$11.72** | **17 / +$11.55** | 36 / +$1.55 | 17 / −$1.93 |
| 150–179 sec | 39 / −$2.73 | **26 / −$30.15** | **26 / −$30.15** | **49 / −$15.08** | **26 / +$4.62** |
| 180–209 sec | **71 / +$23.52** | 32 / −$0.25 | 32 / −$0.25 | **79 / +$28.07** | 32 / −$7.66 |
| 210–239 sec | 10 / −$4.05 | 3 / +$1.63 | 3 / +$1.63 | 11 / −$2.33 | 3 / +$0.60 |

Among buckets with at least ten fills, champion's best/worst were **180–209** (+$23.52/71) and **210–239** (−$4.05/10). Its absolute lowest bucket, 30–59, had only eight trades (−$5.05). New hold and learned sell were best at **120–149** and worst at **150–179**. Composition was best at **180–209** and worst at **150–179**. Fixed exit reversed the new hold pattern: best at **150–179** (+$4.62/26), worst at **180–209** (−$7.66/32). This interaction is why a timing filter cannot be inferred from hold PnL alone. See the [interactive age chart](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/diagnostics/entry-age-performance.html).

New hold's 150–179-second loss appeared in **July (7 fills, −$9.43), August (10, −$6.85), and September (9, −$13.88)**; **UP lost $12.35/12** and **DOWN lost $17.80/14**. Recorded checkpoint paired-book availability was similar at 120, 150, and 180 seconds (about 80% on buy-policy-active days), reducing concern about a uniquely missing 150-second book. That metric excludes absent checkpoints; market and policy selection still confound the comparison. The exact age × month × side cells are in `deep-dive.json`.

## UTC hour, month, and side

Hours are **market start hours in UTC**, not local time or the precise entry second. The best/worst ranking below requires at least five fills in an hour. The [UTC-hour chart](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/diagnostics/utc-hour-performance.html) and `time-slices.parquet` contain all 24 hours with count, PnL, and PF.

| Ledger | Strongest qualifying hours | Weakest qualifying hours |
| --- | --- | --- |
| Frozen champion | 14:00: 9 / +$13.47; 20:00: 10 / +$12.70 | 09:00: 7 / −$12.93; 12:00: 10 / −$10.35 |
| New buy and hold | 20:00: 5 / +$12.03; 23:00: 11 / +$3.23 | 00:00: 8 / −$12.00; 05:00: 5 / −$9.43 |
| Same buys, learned sell | 20:00: 5 / +$12.03; 23:00: 11 / +$3.23 | 00:00: 8 / −$12.00; 05:00: 5 / −$9.43 |
| Champion plus specialist | 20:00: 11 / +$14.72; 14:00: 9 / +$13.47 | 09:00: 8 / −$16.25; 12:00: 11 / −$12.03 |
| Fixed-exit control | 23:00: 11 / +$4.83; 20:00: 5 / +$2.60 | 00:00: 8 / −$5.10; 05:00: 5 / −$2.20 |

Source paired-book availability varied by UTC hour on buy-policy-active days: about **67% of recorded checkpoints at 09:00**, versus **85% at 14:00** and **86% at 20:00**. These small, uneven samples are diagnostic, not evidence that hour of day caused PnL.

| Ledger | July fills / stressed | August fills / stressed | September fills / stressed | UP fills / stressed | DOWN fills / stressed |
| --- | ---: | ---: | ---: | ---: | ---: |
| Frozen champion | 42 / +$34.95 | 77 / +$5.48 | 74 / −$20.75 | 74 / +$12.06 | 119 / +$7.62 |
| New buy and hold | 26 / +$0.40 | 45 / −$4.73 | 29 / −$8.78 | 46 / −$13.45 | 54 / +$0.35 |
| Same buys, learned sell | 26 / +$0.23 | 45 / −$4.73 | 29 / −$8.78 | 46 / −$13.45 | 54 / +$0.18 |
| Champion plus specialist | 56 / +$22.25 | 102 / +$2.36 | 76 / −$20.65 | 97 / −$8.02 | 137 / +$11.97 |
| Fixed-exit control | 26 / +$7.78 | 45 / −$2.53 | 29 / −$3.50 | 46 / −$1.31 | 54 / +$3.05 |

The champion's July gain more than offset its September loss. The new hold model was nearly flat in July and negative in the next two months. New-hold test folds were mixed: fold 13 earned +$5.67/33 fills, while fold 18 lost −$13.43/13 and fold 14 lost −$9.30/8. The candidate traded in seven represented weeks, short of the agreed eight-week minimum. Hour and month patterns were selected after viewing this evaluation and should be specified in advance for a future test.

## Full-range daily graphics and extremes

Each chart covers **all 73 UTC dates**, with daily fill count, daily net/stressed PnL, cumulative net/stressed PnL, and a 73-row daily table. Green trade bars mean full-coverage active days; red shading marks partial/missing source coverage; orange shading marks buy-policy-disabled periods. Dotted lines mark evaluation fold boundaries. Zero on a covered active day is a verified no-trade day; zero during missing or disabled periods is not evidence of selectivity.

| Ledger | Full daily chart | Best trading day | Worst trading day | Best fully covered day | Worst fully covered day |
| --- | --- | ---: | ---: | ---: | ---: |
| Frozen champion | [chart](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/diagnostics/daily-performance-champion.html) | Aug 2: 6 / +$13.85 (partial) | Aug 25: 9 / −$12.78 (covered) | Jul 30: +$10.45 | Aug 25: −$12.78 |
| New buy and hold | [chart](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/diagnostics/daily-performance-buy_hold.html) | Sep 21: 7 / +$7.62 (covered) | Aug 4: 3 / −$8.98 (partial) | Sep 21: +$7.62 | Sep 23: −$4.75 |
| Same buys, learned sell | [chart](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/diagnostics/daily-performance-buy_sell.html) | Sep 21: 7 / +$7.62 (covered) | Aug 4: 3 / −$8.98 (partial) | Sep 21: +$7.62 | Sep 23: −$4.75 |
| Champion plus specialist | [chart](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/diagnostics/daily-performance-champion_specialist.html) | Aug 2: 6 / +$13.85 (partial) | Aug 25: 9 / −$12.78 (covered) | Jul 26: +$7.72 | Aug 25: −$12.78 |
| Fixed-exit control | [chart](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/diagnostics/daily-performance-fixed_exit.html) | Jul 15: 3 / +$4.75 (partial) | Sep 9: 4 / −$6.65 (partial) | Sep 11: +$2.95 | Jul 30: −$2.72 |

The [daily comparison heatmap](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/diagnostics/daily-performance-comparison.html) makes coincident wins/losses visible. Additional strong/weak champion days were Jul 30 (+$10.45) and Sep 10 (−$7.12, partial); new hold's next best/worst days were Aug 7 (+$5.53, covered) and Sep 9 (−$5.05, partial). All dates and coverage flags are in `daily-economics.parquet`.

All 73 days had some books, but only **41** met the agreed threshold of at least 280 complete markets and 95% paired checkpoint coverage. Champion traded on 36 of its 41 fully covered days. The new policy was enabled on 45 days, only 21 of which were fully covered; it traded on 17 of those, and on 33 days in the whole calendar. Its 2.86 buys per fully covered active day remains below the desired 5–8. The prior [chronological activity chart](/Volumes/docker-data/capitonic-btc-directional-model/entry-disposition-tournament-20260924T225644Z/diagnostics/trade-activity-buy_hold.html) and burst ledger preserve quiet-period and burst detail.

## Model-by-model conclusion

- **Champion:** Its modest winning-trade advantage came from price/payout economics, concentrated in 180–209-second entries. July strength, September weakness, large drawdown, and overlap with its training history limit confidence.
- **New buy and hold:** The large 150–179-second loss pocket and weaker UP-side economics explain more than its aggregate −$13.10. The positive 120–149-second bucket is a hypothesis for a future independent test, not a current policy.
- **New buy and sell:** It was able to sell early under the common rules, but did so **once**: a DOWN buy at second 130 sold at second 210, earning $1.70 stressed rather than $1.88 held. This run mostly measures the shared entry policy; it does not meaningfully test an active-selling strategy.
- **Champion plus specialist:** It increased frequency to 234 trades but the specialist branch lost $24.10 and its entries displaced 21 later champion buys. The frequency increase did not preserve expectancy.
- **Fixed-exit control:** It turned the identical 100 buys into +$1.74 stressed with 77 sales and 58 profitable trades, but PF 1.03 and 1.34 average wins per average loss are not enough to qualify.

**Decision:** none of the three challengers passes the agreed 78% trade-win, 68% Wilson lower bound, 1.5 stressed PF, or 5–8 buys per fully covered active day objectives. The evidence supports targeted hypotheses about entry timing, entry price, and exit activation rather than the blanket statement that the models simply failed. Those hypotheses need a prospectively specified walk-forward or paper comparison before any model change or deployment. No training, backfill, process configuration, database schema, image, or deployment was changed by this diagnostic follow-up.
