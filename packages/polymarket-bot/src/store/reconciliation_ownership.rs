use super::*;

impl Store {
    pub(crate) async fn reconciliation_account_process_ids(
        &self,
        account_ref: &str,
    ) -> Result<Vec<Uuid>> {
        let ids = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT process_id FROM polymarket.trading_processes
            WHERE config #>> '{execution,mode}' = 'live'
              AND lower(btrim(config #>> '{execution,account_ref}')) = lower($1)
            LIMIT 4001
        "#,
        )
        .bind(account_ref)
        .fetch_all(&self.pool)
        .await?;
        if ids.len() > 4_000 {
            bail!("reconciliation account process limit exceeded");
        }
        Ok(ids)
    }

    /// Historical token lineage prevents a settled peer redemption from being adopted locally.
    /// Token identity is used only to detect ambiguity, never to prove trade ownership.
    pub(crate) async fn reconciliation_account_token_lineage(
        &self,
        account_ref: &str,
        token_ids: &[String],
    ) -> Result<HashSet<String>> {
        if token_ids.len() > 4_000 {
            bail!("reconciliation token lineage limit exceeded");
        }
        if token_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let rows = sqlx::query_scalar::<_, String>(
            r#"
            SELECT DISTINCT orders.token_id
            FROM polymarket.orders orders
            JOIN polymarket.trading_processes process ON process.process_id = orders.process_id
            WHERE process.config #>> '{execution,mode}' = 'live'
              AND lower(btrim(process.config #>> '{execution,account_ref}')) = lower($1)
              AND orders.token_id = ANY($2::text[])
              AND EXISTS (
                SELECT 1 FROM polymarket.fills fill
                WHERE fill.order_id = orders.order_id
                  AND fill.process_id = orders.process_id
                  AND fill.source = 'live'
              )
            LIMIT 4001
        "#,
        )
        .bind(account_ref)
        .bind(token_ids)
        .fetch_all(&self.pool)
        .await?;
        if rows.len() > 4_000 {
            bail!("reconciliation token lineage limit exceeded");
        }
        Ok(rows.into_iter().collect())
    }

    pub(crate) async fn reconciliation_order_process_ids(
        &self,
        order_ids: &[String],
    ) -> Result<HashMap<String, Uuid>> {
        if order_ids.len() > 8_000 {
            bail!("reconciliation order owner limit exceeded");
        }
        let rows = sqlx::query_as::<_, (String, Uuid)>(
            r#"
            SELECT order_id, process_id FROM polymarket.orders WHERE order_id = ANY($1::text[])
        "#,
        )
        .bind(order_ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().collect())
    }
}
