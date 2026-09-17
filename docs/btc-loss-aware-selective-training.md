# BTC loss-aware selective model training

## Objective

Train one new model bundle from scratch on the same immutable March 21–September 14 coverage.
The bundle contains a directional probability head and a causal loss-risk admission head. It
must retain the prior model's favorable accuracy, payoff and recovery properties while reducing
repeated-loss exposure outside the May 10 signature that motivated the design.

No prior estimator weights, calibrators or fitted artifacts are reused. Historical dates are not
permanently sealed or excluded: expanding chronological cross-fitting supplies leakage-free
evaluation, and the final heads refit from the complete eligible coverage.

## Coverage and inputs

- Directional source: 50,602 markets / 1,872,274 rows / 37 checkpoints per market.
- Economic and risk supervision: authentic VWAP5 cohort only.
- Directional features: the frozen price-only Binance one-second-derived contract.
- Risk features: causal market state, directional probability, conservative confidence, VWAP5,
  stressed edge, decision time and the prespecified volatility/momentum/recovery signature.
- Forbidden: RTDS, Chainlink RefPrice/candles, Kraken, trade prints, L2, open interest, future
  state and exact market identity.

## Cross-fitting

1. Refit the directional head from scratch in expanding chronological folds.
2. Preserve raw and calibrated out-of-fold probabilities for every evaluated row.
3. Build one broad, first-qualified candidate per market from those predictions.
4. Label candidate loss and stressed severity; use multi-loss membership only as a training
   weight, never as an inference feature.
5. Train/calibrate the risk head on earlier out-of-fold candidates.
6. Select directional admission and risk thresholds on the next chronological block.
7. Replay the following block once; stitch each market once.
8. Fit final directional and risk estimators from all eligible data using cross-fitted
   probabilities for calibration, then freeze one combined policy.

## Frozen risk hypothesis

The risk head may learn interactions among low/contracting 120-second volatility, weak boundary
velocity, lower confidence, pullback/recovery conflict, low path efficiency, crossings, side,
entry second, price and stressed edge. It cannot use prior unresolved results or a hard-coded
May 10 identifier.

## Required comparison

Compare the new combined policy with the directional-only baseline on identical outer folds.
Report trades, accuracy, Wilson bound, stressed PnL/PF, recovery ratio, drawdown, loss streaks,
loss dollars avoided and win dollars rejected. Repeat the comparison excluding May 10. The new
model fails if improvement depends on those five markets alone.

## Desired outcomes

- at least 60 stitched trades across eight weeks;
- precision at least 0.78 and Wilson lower bound at least 0.68;
- stressed PF at least 1.50 and recovery ratio no greater than 2.0;
- positive stressed PnL after removing the best day;
- longest loss streak no greater than three;
- fewer maximum consecutive losses than the matched baseline;
- avoided loss dollars greater than rejected win dollars;
- stressed PnL no worse than 95% of the matched baseline;
- the same conclusions with May 10 excluded.

Failure produces a diagnostic artifact only. No export, process, deployment or merge is implied.
