#!/usr/bin/env python3
"""Select immutable AWS versions and consumer-specific rollout tokens without exposing values."""
import argparse
import base64
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[2]
COMPONENTS = ('ingester', 'polymarket-bot')


def command(*args):
    return subprocess.check_output(args, text=True)


def load_values(component):
    return json.loads(command('yq', '-o=json', '.', str(ROOT / f'capitonic-helm-chart/environments/production/{component}.yaml')))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--verify', action='store_true')
    parser.add_argument('--component', choices=COMPONENTS)
    args = parser.parse_args()
    components = (args.component,) if args.component else COMPONENTS
    values = {component: load_values(component) for component in components}
    credentials = {component: dict(v.get('credentialRevisions', {})) for component, v in values.items()}
    aws_cache = {}
    changed_consumers = set()
    now = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    for component, configuration in values.items():
        config = configuration['externalSecrets']
        for entry in config['entries']:
            version = entry['version'] if args.verify else None
            cache_key = (config['secretId'], version)
            if cache_key not in aws_cache:
                request = ['aws', 'secretsmanager', 'get-secret-value', '--region', config['region'], '--secret-id', config['secretId'], '--output', 'json']
                if version:
                    request += ['--version-id', version]
                response = json.loads(command(*request))
                aws_cache[cache_key] = (response['VersionId'], json.loads(response['SecretString']))
            selected_version, source = aws_cache[cache_key]
            projected = {}
            for key, property_name in entry['properties'].items():
                value = source.get(property_name)
                if not isinstance(value, str):
                    raise RuntimeError(f"Missing string AWS property {property_name} for {entry['name']}; no secret changed")
                projected[key] = base64.b64encode(value.encode()).decode()
            actual = json.loads(command('kubectl', '-n', 'capitonic', 'get', 'secret', entry['name'], '-o', 'json'))['data']
            if set(actual) != set(projected):
                raise RuntimeError(f"Key set mismatch for {entry['name']}; refusing to discard existing keys")
            differs = any(actual.get(key) != value for key, value in projected.items())
            mapping_requires_version = False
            if not args.verify and entry.get('version') and entry['version'] != selected_version:
                old_key = (config['secretId'], entry['version'])
                if old_key not in aws_cache:
                    old_response = json.loads(command('aws', 'secretsmanager', 'get-secret-value', '--region', config['region'], '--secret-id', config['secretId'], '--version-id', entry['version'], '--output', 'json'))
                    aws_cache[old_key] = (old_response['VersionId'], json.loads(old_response['SecretString']))
                old_source = aws_cache[old_key][1]
                mapping_requires_version = any(old_source.get(prop) != source[prop] for prop in entry['properties'].values())
            if args.verify:
                if differs:
                    raise RuntimeError(f"Selected AWS version does not match {entry['name']}")
                external = json.loads(command('kubectl', '-n', 'capitonic', 'get', 'externalsecret', entry['name'], '-o', 'json'))
                if not all(item['remoteRef']['version'] == 'uuid/' + selected_version for item in external['spec']['data']):
                    raise RuntimeError(f"ExternalSecret version mismatch for {entry['name']}")
                expected_generation = str(external['metadata']['generation']) + '-'
                if not external.get('status', {}).get('syncedResourceVersion', '').startswith(expected_generation):
                    raise RuntimeError(f"ExternalSecret generation not synchronized: {entry['name']}")
                if not any(c['type'] == 'Ready' and c['status'] == 'True' for c in external.get('status', {}).get('conditions', [])):
                    raise RuntimeError(f"ExternalSecret not Ready: {entry['name']}")
            elif differs or mapping_requires_version or not entry.get('version'):
                entry['version'] = selected_version
                entry['refreshAfter'] = now
                if differs:
                    changed_consumers.update(entry['consumers'])
    if args.verify:
        print('Selected AWS JSON properties exactly match all managed Kubernetes Secrets')
        return
    for consumer in changed_consumers:
        owner = 'polymarket-bot' if consumer == 'polymarket-bot' else 'ingester'
        credentials[owner][consumer] = str(uuid.uuid4())
    print(json.dumps({component: {'externalSecrets': values[component]['externalSecrets'], 'credentialRevisions': credentials[component]} for component in components}, sort_keys=True))


if __name__ == '__main__':
    main()
