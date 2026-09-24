{{- define "ingester.commonEnv" -}}
- {name: RUST_LOG, value: info}
- {name: POSTGRES_HOST, value: pgbouncer}
- {name: POSTGRES_PORT, value: "6432"}
- {name: POSTGRES_SSL_MODE, value: disable}
- {name: MARKET_DATA_INGESTER_DB_POOL_CONNECTIONS, value: "1"}
- {name: MARKET_DATA_INGESTER_CONTROL_DB_POOL_CONNECTIONS, value: "1"}
- {name: MARKET_DATA_INGESTER_DB_ACQUIRE_TIMEOUT_SECS, value: "10"}
- {name: MARKET_DATA_INGESTER_CONTROL_DB_ACQUIRE_TIMEOUT_SECS, value: "3"}
- {name: INGESTER_ADMIN_TOKEN, valueFrom: {secretKeyRef: {name: ingester-auth, key: admin-token}}}
{{- end -}}
