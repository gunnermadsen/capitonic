#!/usr/bin/env node

import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";

const REQUIRED_KEYS = [
  "POSTGRES_PASSWORD",
  "CAPITONIC_TRADING_POSTGRES_PASSWORD",
  "CAPITONIC_INGESTER_MASTER_POSTGRES_PASSWORD",
  "CAPITONIC_INGESTER_WORKER_POSTGRES_PASSWORD",
  "CAPITONIC_GRAFANA_POSTGRES_PASSWORD",
  "GITHUB_USERNAME",
  "GITHUB_PAT",
  "POLYMARKET_HTTP_ADMIN_TOKEN",
  "MARKET_DATA_INGESTER_ADMIN_TOKEN",
  "POLYMARKET_CLOB_API_KEY",
  "POLYMARKET_CLOB_SECRET",
  "POLYMARKET_CLOB_PASSPHRASE",
  "POLYMARKET_PRIVATE_KEY",
  "POLYMARKET_FUNDER_ADDRESS",
  "POLYMARKET_SIGNATURE_TYPE",
  "GRAFANA_ADMIN_USER",
  "GRAFANA_ADMIN_PASSWORD",
  "ARGOCD_ADMIN_PASSWORD",
  "ARGOCD_ADMIN_PASSWORD_HASH",
  "ARGOCD_ADMIN_PASSWORD_MTIME",
  "PROMETHEUS_BASIC_AUTH_USER",
  "PROMETHEUS_BASIC_AUTH_PASSWORD",
  "PROMETHEUS_BASIC_AUTH_PASSWORD_HASH",
  "CLOUDFLARE_ACCOUNT_ID",
  "CLOUDFLARE_ZONE_ID",
  "CLOUDFLARE_EMAIL_ADDRESS",
  "CLOUDFLARE_API_TOKEN",
  "CLOUDFLARED_PRODUCTION_TUNNEL_ID",
  "CLOUDFLARED_PRODUCTION_TUNNEL_CREDENTIALS_B64",
];
const KEY_ALIASES = {
  GHCR_USER: "GITHUB_USERNAME",
  GHCR_PAT_TOKEN: "GITHUB_PAT",
  COLOUDFLARED_PRODUCTION_TUNNEL_CREDENTIALS_B64:
    "CLOUDFLARED_PRODUCTION_TUNNEL_CREDENTIALS_B64",
};

function parseArgs(argv) {
  const options = {
    directory: ".", secretId: "capitonic/polymarket-bot/production",
    region: process.env.AWS_REGION || "eu-west-1", profile: "", dryRun: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--directory") options.directory = argv[++index];
    else if (argument === "--secret-id") options.secretId = argv[++index];
    else if (argument === "--region") options.region = argv[++index];
    else if (argument === "--profile") options.profile = argv[++index];
    else if (argument === "--dry-run") options.dryRun = true;
    else if (argument === "--help" || argument === "-h") {
      console.log(`Usage:
  node scripts/sync-aws-secret.mjs [--directory .] [--secret-id NAME] [--region eu-west-1] [--profile NAME] [--dry-run]

Merges every .env* file in the directory except .env.example* and writes one
AWS Secrets Manager JSON secret. Files are applied in lexical order; later files
override duplicate keys.`);
      process.exit(0);
    } else throw new Error(`Unknown argument: ${argument}`);
  }
  return options;
}

function envFiles(directory) {
  const absoluteDirectory = resolve(directory);
  return readdirSync(absoluteDirectory, { withFileTypes: true })
    .filter((entry) => entry.isFile() || entry.isSymbolicLink()).map((entry) => entry.name)
    .filter((name) => name === ".env" || name.startsWith(".env."))
    .filter((name) => !name.startsWith(".env.example") && !name.includes(".example"))
    .sort((left, right) => left === ".env" ? -1 : right === ".env" ? 1 : left.localeCompare(right))
    .map((name) => join(absoluteDirectory, name));
}

function parseEnvFile(pathname) {
  const values = {};
  for (const rawLine of readFileSync(pathname, "utf8").split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const delimiter = rawLine.indexOf("=");
    if (delimiter <= 0) throw new Error(`Invalid line in ${pathname}: ${rawLine}`);
    const key = rawLine.slice(0, delimiter).trim();
    let value = rawLine.slice(delimiter + 1);
    if ((value.startsWith('"') && value.endsWith('"')) ||
        (value.startsWith("'") && value.endsWith("'"))) value = value.slice(1, -1);
    values[key] = value;
  }
  return values;
}

function awsArgs(args, options) {
  if (options.region) args.push("--region", options.region);
  if (options.profile) args.push("--profile", options.profile);
  return args;
}

function runAws(args, options, inherit = false) {
  const result = spawnSync("aws", awsArgs(args, options), {
    encoding: "utf8", stdio: inherit ? "inherit" : "pipe",
  });
  if (result.error) throw result.error;
  return result;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.region !== "eu-west-1") {
    throw new Error("Production secrets must be stored in eu-west-1 (Ireland).");
  }
  const files = envFiles(options.directory);
  if (files.length === 0) throw new Error(`No eligible .env* files found in ${options.directory}`);
  const values = Object.assign({}, ...files.map(parseEnvFile));
  for (const [source, target] of Object.entries(KEY_ALIASES)) {
    if (values[target] == null && values[source] != null) values[target] = values[source];
  }
  const missing = REQUIRED_KEYS.filter((key) => !values[key]);
  if (missing.length > 0) throw new Error(`Missing required keys: ${missing.join(", ")}`);

  console.log(`Loaded ${Object.keys(values).length} keys from ${files.length} files:`);
  for (const file of files) console.log(`- ${basename(file)}`);
  console.log(`Secret ID: ${options.secretId}`);
  console.log(`Region: ${options.region}`);
  if (options.dryRun) return console.log("Dry run only. No AWS changes made.");

  const tempDirectory = mkdtempSync(join(tmpdir(), "polymarket-secret-"));
  const payloadPath = join(tempDirectory, "secret.json");
  writeFileSync(payloadPath, `${JSON.stringify(values)}\n`, { mode: 0o600 });
  try {
    const describe = runAws(["secretsmanager", "describe-secret", "--secret-id", options.secretId], options);
    const command = describe.status === 0
      ? ["secretsmanager", "put-secret-value", "--secret-id", options.secretId]
      : ["secretsmanager", "create-secret", "--name", options.secretId];
    command.push("--secret-string", `file://${payloadPath}`);
    console.log(`${describe.status === 0 ? "Updating" : "Creating"} secret.`);
    const applied = runAws(command, options, true);
    if (applied.status !== 0) process.exit(applied.status ?? 1);
  } finally {
    rmSync(tempDirectory, { recursive: true, force: true });
  }
}

try { main(); } catch (error) { console.error(error.message); process.exit(1); }
