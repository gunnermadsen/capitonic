use super::*;

pub(super) async fn run_grafana_live(
    manager: BtcProcessManager,
    publisher: GrafanaLivePublisher,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut ticker = tokio::time::interval(publisher.publish_interval());
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last_countdown_error: Option<String> = None;
    let mut last_market_path_error: Option<String> = None;
    let mut last_entry_status_error: Option<String> = None;
    info!(
        countdown_channel = polymarket_bot::grafana_live::COUNTDOWN_CHANNEL,
        market_path_channel = polymarket_bot::grafana_live::MARKET_PATH_CHANNEL,
        entry_status_channel = polymarket_bot::grafana_live::ENTRY_STATUS_CHANNEL,
        "Grafana Live BTC market publishers started"
    );
    let mut market_path_state = MarketPathPublicationState::default();
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            _ = ticker.tick() => {
                let snapshot = manager.grafana_countdown_snapshot(Utc::now()).await;
                match publisher.publish(&snapshot).await {
                    Ok(()) => {
                        if last_countdown_error.take().is_some() {
                            info!("Grafana Live BTC market countdown publishing recovered");
                        }
                    }
                    Err(publish_error) => {
                        let message = format!("{publish_error:#}");
                        if last_countdown_error.as_deref() != Some(message.as_str()) {
                            warn!(
                                error = %message,
                                "Grafana Live BTC market countdown publish failed; retrying"
                            );
                            last_countdown_error = Some(message);
                        }
                    }
                }

                let observed_at = Utc::now();
                let market_path_observation = manager
                    .grafana_market_path_observation(observed_at)
                    .await;
                let market_path_result = match market_path_state
                    .observe(observed_at, market_path_observation)
                {
                    Some(snapshot) => publisher.publish_market_path(&snapshot).await,
                    None => Ok(()),
                };
                match market_path_result {
                    Ok(()) => {
                        if last_market_path_error.take().is_some() {
                            info!("Grafana Live BTC market path publishing recovered");
                        }
                    }
                    Err(publish_error) => {
                        let message = format!("{publish_error:#}");
                        if last_market_path_error.as_deref() != Some(message.as_str()) {
                            warn!(
                                error = %message,
                                "Grafana Live BTC market path publish failed; retrying"
                            );
                            last_market_path_error = Some(message);
                        }
                    }
                }

                let entry_status_result = match manager
                    .grafana_entry_status_snapshot(Utc::now())
                    .await
                {
                    Ok(snapshot) => publisher.publish_entry_status(&snapshot).await,
                    Err(error) => Err(error),
                };
                match entry_status_result {
                    Ok(()) => {
                        if last_entry_status_error.take().is_some() {
                            info!("Grafana Live trading entry status publishing recovered");
                        }
                    }
                    Err(publish_error) => {
                        let message = format!("{publish_error:#}");
                        if last_entry_status_error.as_deref() != Some(message.as_str()) {
                            warn!(
                                error = %message,
                                "Grafana Live trading entry status publish failed; retrying"
                            );
                            last_entry_status_error = Some(message);
                        }
                    }
                }
            }
        }
    }
    info!("Grafana Live BTC market publishers stopped");
}
